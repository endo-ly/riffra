#include <algorithm>

#include "TimelineEngine.h"

namespace riffra {

namespace {

constexpr auto kRecordingApplyTimeout = std::chrono::milliseconds(500);

}  // namespace

RealtimeRequest TimelineEngine::startRecording(const int countInBeats, juce::String& error) {
    const auto ready = graphRegistry.access([](const ControlGraphRegistry::State& graphs) {
        return graphs.latestCommitted != nullptr && graphs.latestCommitted->outputSampleRate > 0.0;
    });
    if (!ready) {
        error = "Arrange recording requires a prepared Arrangement Graph.";
        return RealtimeRequest::rejected;
    }
    if (realtimeFrame.read().recordingPhase != RecordingPhase::idle) {
        error = "Arrange recording is already active.";
        return RealtimeRequest::rejected;
    }
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::startRecording;
    command.countInBeats = countInBeats;
    if (!submit(command).has_value()) {
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    {
        const std::lock_guard lock(finalizedRecordingMutex);
        finalizedRecordingTracks.clear();
        finalizedRecordingSampleRate = 0.0;
        finalizedRecordingBlockSize = 0;
    }
    return RealtimeRequest::accepted;
}

void TimelineEngine::startRecordingNow(RealtimeState& state, const int countInBeats) noexcept {
    if (state.graph == nullptr || state.recordingPhase != RecordingPhase::idle) return;
    auto& graph = *state.graph;
    for (auto& track : graph.tracks) recordingCapture->resetTrack(track->runtime->recordingCapture);
    recordingCapture->resetCaptureErrors();
    state.recordingPassOrdinal = 1;
    const auto alreadyPlaying = state.transport == TransportState::playing;
    if (alreadyPlaying || countInBeats <= 0) {
        state.recordingPhase = RecordingPhase::recording;
        state.recordingStartAudioSample = state.audioClockSample;
        const auto requestedSample =
            state.seekPending ? state.pendingSeekSample : state.timelineSample;
        state.recordingStartTick =
            graph.timebase.sampleToTick(requestedSample, graph.outputSampleRate);
        if (!alreadyPlaying) state.transport = TransportState::playing;
    } else {
        state.countInRemainingSamples = graph.beatSamples * std::max(0, countInBeats);
        state.recordingPhase = RecordingPhase::countingIn;
    }
}

bool TimelineEngine::stopRecording(juce::String& error) {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::stopRecording;
    const auto sequence = submit(command);
    if (!sequence.has_value()) {
        error = "The realtime command queue is full.";
        return false;
    }
    if (!waitUntilApplied(*sequence, kRecordingApplyTimeout)) {
        error = "The audio thread did not close the recording capture in time.";
        return false;
    }
    return true;
}

void TimelineEngine::closeRecordingCaptures(RealtimeState& state) noexcept {
    if (state.recordingPhase == RecordingPhase::idle) return;
    state.recordingPhase = RecordingPhase::stopping;
    if (state.graph == nullptr) return;
    for (auto& trackPtr : state.graph->tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        if (!runtime.armed || runtime.instrumentTrack ||
            runtime.recordingCapture.state != RecordingCaptureState::capturing)
            continue;
        (void)recordingCapture->endTrackCapture(track.id, runtime.recordingCapture);
        runtime.recordingCapture.state = RecordingCaptureState::idle;
    }
}

RealtimeRequest TimelineEngine::stopArrangeRecording(juce::String& error) {
    const auto sequence = enqueueArrangeRecordingStop();
    if (!sequence.has_value()) {
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    waitForCommandApplied(*sequence);
    return RealtimeRequest::accepted;
}

std::optional<std::uint64_t> TimelineEngine::enqueueArrangeRecordingStop() noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::stopArrangeRecording;
    return submit(command);
}

bool TimelineEngine::finalizeRecording(juce::String& error) {
    if (controlRecordingSink != nullptr)
        controlRecordingSink->setMidiSourceIds(midiSources.snapshot());
    const std::lock_guard lock(finalizedRecordingMutex);
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    if (controlRecordingSink == nullptr) return true;
    visitActiveGraph(false, [this](const PreparedTimeline& graph, const RealtimeFrame&) {
        finalizedRecordingSampleRate = graph.outputSampleRate;
        finalizedRecordingBlockSize = graph.preparedBlockSize;
        finalizedRecordingTracks.reserve(graph.tracks.size());
        for (const auto& track : graph.tracks) {
            if (track->runtime->instrumentTrack || !track->runtime->armed) continue;
            finalizedRecordingTracks.push_back({track->id, track->effects});
        }
        return true;
    });
    if (recordingCapture->captureErrors() == 0) return true;
    error = "Recording capture reported errors.";
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    return false;
}

bool TimelineEngine::processFinalizedRecording(
    ArrangementCaptureSink* sink, juce::String& error,
    const ProcessingProgressCallback& progress) noexcept {
    if (progress) progress();
    std::vector<OfflineRecordingTrack> tracks;
    double sampleRate;
    int blockSize;
    {
        const std::lock_guard lock(finalizedRecordingMutex);
        sampleRate = finalizedRecordingSampleRate;
        blockSize = finalizedRecordingBlockSize;
        tracks = std::move(finalizedRecordingTracks);
        finalizedRecordingSampleRate = 0.0;
        finalizedRecordingBlockSize = 0;
    }
    const auto generated =
        generateProcessedVariants(sampleRate, blockSize, tracks, sink, error, progress);
    return generated && recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::generateProcessedVariants(
    const double sampleRate, const int preparedBlockSize,
    const std::vector<OfflineRecordingTrack>& tracks, ArrangementCaptureSink* const sink,
    juce::String& error, const ProcessingProgressCallback& progress) noexcept {
    if (sink == nullptr || sampleRate <= 0.0) return true;
    const auto blockSize = std::max(1, preparedBlockSize);
    juce::AudioFormatManager formatReader;
    formatReader.registerBasicFormats();
    for (const auto& track : tracks) {
        if (progress) progress();
        const auto rawFile = sink->prepareRawForReading(track.id);
        if (rawFile == juce::File{}) continue;
        if (progress) progress();
        const auto segments = sink->getRawSegmentRanges(track.id);
        if (segments.empty()) continue;
        // Open the flushed raw file as a stream so that non-.wav extensions
        // (e.g. .partial) are accepted by the AudioFormatManager readers.
        auto rawStream = rawFile.createInputStream();
        if (rawStream == nullptr || !rawStream->openedOk()) return false;
        std::unique_ptr<juce::AudioFormatReader> reader(
            formatReader.createReaderFor(std::move(rawStream)));
        if (reader == nullptr) {
            error = "Recorded raw audio could not be opened for offline processing.";
            return false;
        }
        PluginChain offlineEffects;
        if (progress) progress();
        if (!offlineEffects.load(track.effects, sampleRate, blockSize,
                                 PluginProcessingMode::offline, error,
                                 track.id + "/offline-processing"))
            return false;
        if (progress) progress();
        const auto delay = std::max(0, offlineEffects.latencySamples());
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentLength = segEnd - segStart;
            if (segmentLength > static_cast<std::uint64_t>(std::numeric_limits<int>::max())) {
                error = "Recorded audio segment is too large for offline processing.";
                return false;
            }
            const auto segmentSamples = static_cast<int>(segmentLength);
            if (segmentSamples <= 0) continue;
            offlineEffects.reset();
            juce::AudioBuffer<float> blockBuffer(2, blockSize);
            juce::AudioBuffer<float> processedBlock(2, blockSize);
            int discarded = delay;
            int written = 0;
            if (progress) progress();
            constexpr int kOfflineWriterTimeoutMs = 5000;
            const auto consumeProcessedBlock = [&](const int count) noexcept {
                auto writeOffset = 0;
                if (discarded > 0) {
                    const auto skipped = std::min(discarded, count);
                    discarded -= skipped;
                    writeOffset += skipped;
                }
                const auto writable = std::min(count - writeOffset, segmentSamples - written);
                if (writable <= 0) return true;
                const std::array<const float*, 2> outputChannels{
                    processedBlock.getReadPointer(0) + writeOffset,
                    processedBlock.getReadPointer(1) + writeOffset,
                };
                if (!sink->writeProcessedAudioTrackOffline(track.id, outputChannels.data(),
                                                           writable, kOfflineWriterTimeoutMs))
                    return false;
                written += writable;
                return true;
            };

            // Process raw audio in bounded blocks and write post-latency samples immediately.
            int remaining = segmentSamples;
            std::int64_t readPos = static_cast<std::int64_t>(segStart);
            while (remaining > 0) {
                const auto count = std::min(blockSize, remaining);
                blockBuffer.clear();
                if (!reader->read(blockBuffer.getArrayOfWritePointers(), 2, readPos, count)) {
                    error = "Recorded raw audio could not be read for offline processing.";
                    return false;
                }
                readPos += count;
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count);
                if (!consumeProcessedBlock(count)) return false;
                if (progress) progress();
                remaining -= count;
            }

            // Flush plugin latency with bounded zero blocks until the segment length is written.
            while (written < segmentSamples) {
                const auto count = blockSize;
                blockBuffer.clear();
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count);
                if (!consumeProcessedBlock(count)) return false;
                if (progress) progress();
            }
        }
    }
    return true;
}

juce::var TimelineEngine::recordingConfiguration() const {
    const auto timelineSample = realtimeFrame.read().timelineSample;
    return graphRegistry.access([timelineSample](const ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.latestCommitted;
        if (graph == nullptr) return juce::var();
        auto* result = new juce::DynamicObject();
        result->setProperty("sampleRate", graph->outputSampleRate);
        const auto tick = static_cast<juce::int64>(
            graph->timebase.sampleToTick(timelineSample, graph->outputSampleRate));
        result->setProperty("timelineStartTick", tick);
        result->setProperty("loopEnabled", graph->loopEnabled);
        result->setProperty("loopStartSample", static_cast<juce::int64>(graph->loopStartSample));
        result->setProperty("loopEndSample", static_cast<juce::int64>(graph->loopEndSample));
        result->setProperty("punchEnabled", graph->punchEnabled);
        result->setProperty("punchStartSample", static_cast<juce::int64>(graph->punchStartSample));
        result->setProperty("punchEndSample", static_cast<juce::int64>(graph->punchEndSample));
        juce::Array<juce::var> trackValues;
        for (const auto& track : graph->tracks) {
            if (!track->runtime->armed) continue;
            auto* value = new juce::DynamicObject();
            value->setProperty("trackId", track->id);
            value->setProperty("kind", track->runtime->instrumentTrack ? "instrument" : "audio");
            value->setProperty("audioInputChannel", track->runtime->audioInputChannel);
            value->setProperty("midiDeviceId", track->runtime->midiDeviceId);
            value->setProperty("midiChannel", track->runtime->midiChannel);
            value->setProperty("pluginLatencySamples",
                               static_cast<int>(track->runtime->pluginDelaySamples));
            value->setProperty("pluginTailSamples",
                               static_cast<int>(track->runtime->pluginTailSamples));
            trackValues.add(juce::var(value));
        }
        result->setProperty("tracks", trackValues);
        return juce::var(result);
    });
}

bool TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    if (controlRecordingSink != nullptr || sink == nullptr) return false;
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::setRecordingSink;
    command.recordingSink = sink;
    if (!submit(command).has_value()) return false;
    controlRecordingSink = sink;
    return true;
}

bool TimelineEngine::clearRecordingSink() noexcept {
    if (controlRecordingSink == nullptr) return true;
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::clearRecordingSink;
    if (!submit(command).has_value()) return false;
    if (!recordingCapture->waitForRetiredSink(controlRecordingSink)) return false;
    controlRecordingSink = nullptr;
    return true;
}

void TimelineEngine::advanceCountIn(RealtimeState& state, const int sampleCount) noexcept {
    state.captureBlockOffset = 0;
    state.captureBlockSamples = 0;
    state.playbackBlockOffset = 0;
    state.countInBlockStartRemainingSamples = 0;
    if (sampleCount <= 0) return;
    const auto* graph = state.graph;
    if (state.recordingPhase == RecordingPhase::idle ||
        state.recordingPhase == RecordingPhase::stopping)
        return;
    auto sampleOffset = 0;
    auto capturedSamples = sampleCount;
    auto transitionedFromCountIn = false;
    if (state.recordingPhase == RecordingPhase::countingIn) {
        const auto remaining = state.countInRemainingSamples;
        state.countInBlockStartRemainingSamples = remaining;
        if (remaining >= sampleCount) {
            state.countInRemainingSamples = remaining - sampleCount;
            return;
        }
        // The count-in ends inside this block. Commands of the block were
        // applied first, so a Stop in the same block has already cancelled it.
        sampleOffset = static_cast<int>(std::max<std::int64_t>(0, remaining));
        state.playbackBlockOffset = sampleOffset;
        capturedSamples = sampleCount - sampleOffset;
        state.countInRemainingSamples = 0;
        state.recordingStartAudioSample =
            state.audioClockSample + static_cast<std::uint64_t>(sampleOffset);
        if (graph != nullptr)
            state.recordingStartTick =
                graph->timebase.sampleToTick(state.timelineSample, graph->outputSampleRate);
        state.transport = TransportState::playing;
        state.recordingPhase = RecordingPhase::recording;
        transitionedFromCountIn = true;
    }

    if (graph == nullptr || !graph->punchEnabled) {
        state.captureBlockOffset = sampleOffset;
        state.captureBlockSamples = capturedSamples;
        return;
    }

    const auto position = state.seekPending ? state.pendingSeekSample : state.timelineSample;
    const auto playbackOffset = transitionedFromCountIn ? sampleOffset : 0;
    const auto playbackSamples = sampleCount - playbackOffset;
    const auto blockEnd = position + static_cast<std::int64_t>(playbackSamples);
    if (blockEnd <= graph->punchStartSample || position >= graph->punchEndSample) return;
    const auto punchOffset =
        static_cast<int>(std::max<std::int64_t>(0, graph->punchStartSample - position));
    const auto end = std::min<std::int64_t>(blockEnd, graph->punchEndSample);
    state.captureBlockOffset = playbackOffset + punchOffset;
    state.captureBlockSamples =
        static_cast<int>(std::max<std::int64_t>(0, end - position - punchOffset));
}

}  // namespace riffra
