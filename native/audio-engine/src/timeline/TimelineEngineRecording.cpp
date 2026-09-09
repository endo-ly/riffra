#include "TimelineEngine.h"

#include <algorithm>

namespace riffra {

bool TimelineEngine::startRecording(const int countInBeats, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr || timeline->outputSampleRate <= 0.0) {
        error = "Arrange recording requires a prepared Arrangement Graph.";
        return false;
    }
    if (recordingPhase.load(std::memory_order_acquire) != RecordingPhase::idle) {
        error = "Arrange recording is already active.";
        return false;
    }
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    for (auto& track : timeline->tracks) {
        recordingCapture->resetTrack(track->runtime->recordingCapture);
    }
    recordingCapture->resetCaptureErrors();
    recordingPassOrdinal.store(1, std::memory_order_release);
    const auto alreadyPlaying = state.load(std::memory_order_acquire) == State::playing;
    if (alreadyPlaying || countInBeats <= 0) {
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        recordingStartAudioSample.store(audioClockSample.load(std::memory_order_acquire),
                                        std::memory_order_release);
        const auto tick = timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate);
        recordingStartTick.store(tick, std::memory_order_release);
        if (!alreadyPlaying) state.store(State::playing, std::memory_order_release);
    } else {
        countInRemainingSamples.store(timeline->beatSamples * std::max(0, countInBeats),
                                      std::memory_order_release);
        recordingPhase.store(RecordingPhase::countingIn, std::memory_order_release);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::stopRecording() noexcept {
    recordingPhase.store(RecordingPhase::stopping, std::memory_order_release);
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    const auto hasCaptureWork =
        timeline != nullptr &&
        std::any_of(timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return recordingCapture->hasCaptureWork(track->runtime->recordingCapture);
        });
    if (!hasCaptureWork) recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::cancelRecordingIfCountingIn() noexcept {
    auto expected = RecordingPhase::countingIn;
    if (!recordingPhase.compare_exchange_strong(expected, RecordingPhase::idle,
                                                std::memory_order_acq_rel))
        return false;
    countInRemainingSamples.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::finalizeRecording(juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    auto sinkLease = recordingCapture->acquireSink();
    auto* sink = sinkLease.get();
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    if (timeline == nullptr || sink == nullptr) {
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
        return true;
    }
    finalizedRecordingSampleRate = timeline->outputSampleRate;
    finalizedRecordingBlockSize = timeline->preparedBlockSize;
    finalizedRecordingTracks.reserve(timeline->tracks.size());
    for (const auto& track : timeline->tracks) {
        if (track->runtime == nullptr || track->runtime->instrumentTrack || !track->runtime->armed)
            continue;
        finalizedRecordingTracks.push_back({track->id, track->effectState});
    }
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (!track.runtime->armed || track.runtime->instrumentTrack ||
            track.runtime->recordingCapture.state != RecordingCaptureState::capturing)
            continue;
        if (!recordingCapture->endTrackCapture(track.id, track.runtime->recordingCapture)) {
            error = "Recording Capture Segment could not be closed.";
            track.runtime->recordingCapture.state = RecordingCaptureState::idle;
            finalizedRecordingTracks.clear();
            finalizedRecordingSampleRate = 0.0;
            finalizedRecordingBlockSize = 0;
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return false;
        }
        track.runtime->recordingCapture.state = RecordingCaptureState::idle;
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    return recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::processFinalizedRecording(juce::String& error) noexcept {
    return processFinalizedRecording(nullptr, error);
}

bool TimelineEngine::processFinalizedRecording(
    ArrangementCaptureSink* sink, juce::String& error,
    const ProcessingProgressCallback& progress) noexcept {
    if (progress) progress();
    std::vector<OfflineRecordingTrack> tracks;
    double sampleRate;
    int blockSize;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        sampleRate = finalizedRecordingSampleRate;
        blockSize = finalizedRecordingBlockSize;
        tracks = std::move(finalizedRecordingTracks);
        finalizedRecordingSampleRate = 0.0;
        finalizedRecordingBlockSize = 0;
    }

    if (sink == nullptr) {
        auto sinkLease = recordingCapture->acquireSink();
        sink = sinkLease.get();
        if (sink == nullptr) return true;
        const auto generated =
            generateProcessedVariants(sampleRate, blockSize, tracks, sink, error, progress);
        return generated && recordingCapture->captureErrors() == 0;
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
        if (!offlineEffects.load(track.effectState, sampleRate, blockSize, error,
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
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return {};
    auto* result = new juce::DynamicObject();
    result->setProperty("sampleRate", timeline->outputSampleRate);
    const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
        timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate));
    result->setProperty("timelineStartTick", tick);
    result->setProperty("loopEnabled", timeline->loopEnabled);
    result->setProperty("loopStartSample", static_cast<juce::int64>(timeline->loopStartSample));
    result->setProperty("loopEndSample", static_cast<juce::int64>(timeline->loopEndSample));
    result->setProperty("punchEnabled", timeline->punchEnabled);
    result->setProperty("punchStartSample", static_cast<juce::int64>(timeline->punchStartSample));
    result->setProperty("punchEndSample", static_cast<juce::int64>(timeline->punchEndSample));
    juce::Array<juce::var> trackValues;
    for (const auto& track : timeline->tracks) {
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
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingCapture->setSink(sink);
}

void TimelineEngine::clearRecordingSink() noexcept { recordingCapture->clearSink(); }

bool TimelineEngine::recordingWindow(const int sampleCount, int& sampleOffset,
                                     int& capturedSamples) noexcept {
    sampleOffset = 0;
    capturedSamples = std::max(0, sampleCount);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    if (sampleCount <= 0) return false;
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    auto phase = recordingPhase.load(std::memory_order_acquire);
    auto transitionedFromCountIn = false;
    if (phase == RecordingPhase::idle || phase == RecordingPhase::stopping) {
        capturedSamples = 0;
        return false;
    }
    if (phase == RecordingPhase::countingIn) {
        const auto remaining = countInRemainingSamples.load(std::memory_order_acquire);
        countInBlockStartRemainingSamples.store(remaining, std::memory_order_release);
        if (remaining >= sampleCount) {
            countInRemainingSamples.store(remaining - sampleCount, std::memory_order_release);
            capturedSamples = 0;
            return false;
        }
        sampleOffset = static_cast<int>(std::max<std::int64_t>(0, remaining));
        playbackBlockOffset.store(sampleOffset, std::memory_order_release);
        capturedSamples = sampleCount - sampleOffset;
        countInRemainingSamples.store(0, std::memory_order_release);
        recordingStartAudioSample.store(audioClockSample.load(std::memory_order_acquire) +
                                            static_cast<std::uint64_t>(sampleOffset),
                                        std::memory_order_release);
        if (active != nullptr) {
            const auto tick = active->timebase.sampleToTick(
                timelineSample.load(std::memory_order_acquire), active->outputSampleRate);
            recordingStartTick.store(tick, std::memory_order_release);
        }
        state.store(State::playing, std::memory_order_release);
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        phase = RecordingPhase::recording;
        transitionedFromCountIn = true;
    }

    if (active == nullptr || !active->punchEnabled) {
        captureBlockOffset.store(sampleOffset, std::memory_order_release);
        captureBlockSamples.store(capturedSamples, std::memory_order_release);
        return true;
    }

    const auto position = timelineSample.load(std::memory_order_acquire);
    const auto playbackOffset = transitionedFromCountIn ? sampleOffset : 0;
    const auto playbackSamples = sampleCount - playbackOffset;
    const auto blockEnd = position + static_cast<std::int64_t>(playbackSamples);
    if (blockEnd <= active->punchStartSample || position >= active->punchEndSample) {
        capturedSamples = 0;
        return false;
    }
    const auto punchOffset =
        static_cast<int>(std::max<std::int64_t>(0, active->punchStartSample - position));
    sampleOffset = playbackOffset + punchOffset;
    const auto end = std::min<std::int64_t>(blockEnd, active->punchEndSample);
    capturedSamples = static_cast<int>(std::max<std::int64_t>(0, end - position - punchOffset));
    captureBlockOffset.store(sampleOffset, std::memory_order_release);
    captureBlockSamples.store(capturedSamples, std::memory_order_release);
    return capturedSamples > 0;
}

}  // namespace riffra

