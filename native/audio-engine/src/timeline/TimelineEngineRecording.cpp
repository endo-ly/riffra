#include <algorithm>

#include "TimelineEngine.h"
#include "recording/ArrangeRecordingSession.h"

namespace riffra {

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
        if (!alreadyPlaying) {
            state.transport = TransportState::playing;
            state.graph->beginAutomationRead();
        }
    } else {
        const auto requestedSample =
            state.seekPending ? state.pendingSeekSample : state.timelineSample;
        const auto tick = graph.timebase.sampleToTick(requestedSample, graph.outputSampleRate);
        const auto meter = graph.timebase.meterAt(static_cast<double>(tick));
        state.countInBeatSamples = std::max<std::int64_t>(
            1, static_cast<std::int64_t>(
                   std::llround(graph.outputSampleRate * 60.0 * 4.0 /
                                (graph.timebase.tempoAt(tick) * meter.denominator))));
        state.countInRemainingSamples = state.countInBeatSamples * std::max(0, countInBeats);
        state.recordingPhase = RecordingPhase::countingIn;
    }
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
            OfflineRecordingTrack offline{track->id, track->effects, {}, {}};
            for (const auto& effect : track->effects) {
                auto& chain = track->runtime->effects();
                offline.mainInputDelays.emplace_back(
                    effect.id, track->runtime->effectAlignment.delaySamples(effect.id));
                if (chain.sidechainBuffer(effect.id) != nullptr)
                    offline.sidechainInputs.push_back({"capture:" + effect.id,
                                                       {},
                                                       effect.id,
                                                       0.0,
                                                       SendPositionSpec::postFader,
                                                       true});
            }
            finalizedRecordingTracks.push_back(std::move(offline));
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
        std::vector<std::unique_ptr<juce::AudioFormatReader>> auxiliaryReaders;
        bool replayable = true;
        for (const auto& input : track.sidechainInputs) {
            const auto file = sink->prepareSidechainForReading(track.id, input.targetDeviceId);
            auto auxiliary = std::unique_ptr<juce::AudioFormatReader>(
                file == juce::File{} ? nullptr : formatReader.createReaderFor(file));
            if (auxiliary == nullptr || auxiliary->numChannels != 2 ||
                auxiliary->sampleRate != sampleRate ||
                auxiliary->lengthInSamples != reader->lengthInSamples) {
                sink->processedVariantUnavailable(
                    track.id, "sidechain input for device " + input.targetDeviceId +
                                  " is unavailable or incomplete; the raw take is retained");
                replayable = false;
                break;
            }
            auxiliaryReaders.push_back(std::move(auxiliary));
        }
        if (!replayable) continue;
        PluginChain offlineEffects;
        if (progress) progress();
        if (!offlineEffects.load(track.effects, track.sidechainInputs, sampleRate, blockSize,
                                 PluginProcessingMode::offline, error,
                                 track.id + "/offline-processing"))
            return false;
        std::vector<juce::String> deviceIds;
        deviceIds.reserve(track.effects.size());
        for (const auto& effect : track.effects) deviceIds.push_back(effect.id);
        PluginChainAlignment offlineAlignment;
        if (!offlineAlignment.prepare(deviceIds, track.mainInputDelays, blockSize, error))
            return false;
        if (progress) progress();
        const auto totalDelay = static_cast<std::int64_t>(offlineEffects.latencySamples()) +
                                offlineAlignment.latencySamples();
        if (totalDelay > std::numeric_limits<int>::max()) {
            error = "recorded sidechain alignment exceeds the supported buffer size";
            return false;
        }
        const auto delay = static_cast<int>(std::max<std::int64_t>(0, totalDelay));
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentLength = segEnd - segStart;
            if (segmentLength > static_cast<std::uint64_t>(std::numeric_limits<int>::max())) {
                error = "Recorded audio segment is too large for offline processing.";
                return false;
            }
            const auto segmentSamples = static_cast<int>(segmentLength);
            if (segmentSamples <= 0) continue;
            offlineEffects.reset();
            offlineAlignment.reset();
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
                if (!reader->read(&blockBuffer, 0, count, readPos, true, true)) {
                    error = "Recorded raw audio could not be read for offline processing.";
                    return false;
                }
                readPos += count;
                offlineEffects.clearSidechainBuffers();
                for (std::size_t index = 0; index < auxiliaryReaders.size(); ++index) {
                    auto* buffer =
                        offlineEffects.sidechainBuffer(track.sidechainInputs[index].targetDeviceId);
                    if (buffer == nullptr ||
                        !auxiliaryReaders[index]->read(buffer->getArrayOfWritePointers(), 2,
                                                       readPos - count, count)) {
                        error = "recorded sidechain input could not be read for offline processing";
                        return false;
                    }
                }
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count, nullptr,
                                       {}, 0, false, &offlineAlignment);
                if (!consumeProcessedBlock(count)) return false;
                if (progress) progress();
                remaining -= count;
            }

            // Flush plugin latency with bounded zero blocks until the segment length is written.
            while (written < segmentSamples) {
                const auto count = blockSize;
                blockBuffer.clear();
                offlineEffects.clearSidechainBuffers();
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count, nullptr,
                                       {}, 0, false, &offlineAlignment);
                if (!consumeProcessedBlock(count)) return false;
                if (progress) progress();
            }
        }
    }
    return true;
}

RecordingConfiguration TimelineEngine::recordingConfiguration() const {
    const auto timelineSample = realtimeFrame.read().timelineSample;
    return graphRegistry.access([timelineSample](const ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.latestCommitted;
        RecordingConfiguration result;
        if (graph == nullptr) return result;
        result.sampleRate = graph->outputSampleRate;
        result.timelineStartTick =
            graph->timebase.sampleToTick(timelineSample, graph->outputSampleRate);
        result.loopEnabled = graph->loopEnabled;
        result.loopStartSample = static_cast<juce::int64>(graph->loopStartSample);
        result.loopEndSample = static_cast<juce::int64>(graph->loopEndSample);
        result.punchEnabled = graph->punchEnabled;
        result.punchStartSample = static_cast<juce::int64>(graph->punchStartSample);
        result.punchEndSample = static_cast<juce::int64>(graph->punchEndSample);

        for (const auto& track : graph->tracks) {
            if (!track->runtime->armed) continue;
            RecordingTrackConfiguration value;
            value.trackId = track->id;
            value.kind =
                track->runtime->instrumentTrack ? TrackKindSpec::instrument : TrackKindSpec::audio;
            value.audioInputChannel = track->runtime->audioInputChannel;
            value.midiDeviceId = track->runtime->midiDeviceId;
            value.midiChannel = track->runtime->midiChannel;
            value.pluginLatencySamples = static_cast<int>(track->runtime->pluginDelaySamples);
            value.pluginTailSamples = static_cast<int>(track->runtime->pluginTailSamples);

            for (const auto& effect : track->effects) {
                if (track->runtime->effects().sidechainBuffer(effect.id) != nullptr)
                    value.sidechainInputs.push_back(effect.id);
            }

            result.tracks.push_back(std::move(value));
        }

        return result;
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
        if (state.graph != nullptr) state.graph->beginAutomationRead();
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
