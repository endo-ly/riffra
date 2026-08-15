#include "RecordingCaptureRuntime.h"

#include <algorithm>
#include <array>
#include <limits>
#include <thread>

namespace riffra {

RecordingCaptureRuntime::SinkLease::SinkLease(RecordingCaptureRuntime& ownerToUse,
                                              ArrangementCaptureSink* const sinkToUse) noexcept
    : owner(&ownerToUse), sink(sinkToUse) {}

RecordingCaptureRuntime::SinkLease::SinkLease(SinkLease&& other) noexcept
    : owner(other.owner), sink(other.sink) {
    other.owner = nullptr;
    other.sink = nullptr;
}

RecordingCaptureRuntime::SinkLease& RecordingCaptureRuntime::SinkLease::operator=(
    SinkLease&& other) noexcept {
    if (this == &other) return *this;
    release();
    owner = other.owner;
    sink = other.sink;
    other.owner = nullptr;
    other.sink = nullptr;
    return *this;
}

RecordingCaptureRuntime::SinkLease::~SinkLease() { release(); }

void RecordingCaptureRuntime::SinkLease::release() noexcept {
    if (owner == nullptr) return;
    owner->recordingSinkReaders.fetch_sub(1, std::memory_order_acq_rel);
    owner = nullptr;
    sink = nullptr;
}

void RecordingCaptureRuntime::setSink(ArrangementCaptureSink* const sink) noexcept {
    recordingSink.store(sink, std::memory_order_release);
}

void RecordingCaptureRuntime::clearSink() noexcept {
    recordingSink.store(nullptr, std::memory_order_release);
    while (recordingSinkReaders.load(std::memory_order_acquire) != 0) std::this_thread::yield();
}

RecordingCaptureRuntime::SinkLease RecordingCaptureRuntime::acquireSink() noexcept {
    // Increment before loading the pointer. This closes the race where clearSink()
    // could observe zero readers between the load and the increment.
    recordingSinkReaders.fetch_add(1, std::memory_order_acq_rel);
    return SinkLease(*this, recordingSink.load(std::memory_order_acquire));
}

void RecordingCaptureRuntime::resetTrack(RecordingCaptureTrackState& track) noexcept {
    track.reset();
}

bool RecordingCaptureRuntime::hasCaptureWork(
    const RecordingCaptureTrackState& track) const noexcept {
    return track.state == RecordingCaptureState::capturing ||
           track.state == RecordingCaptureState::drainingTail;
}

bool RecordingCaptureRuntime::beginTrackCapture(const juce::String& trackId,
                                                RecordingCaptureTrackState& track,
                                                const std::uint64_t audioStartSample,
                                                const std::uint64_t timelineStartSample) noexcept {
    auto sink = acquireSink();
    if (!sink || !sink->beginAudioTrackCapture(trackId, audioStartSample, timelineStartSample)) {
        track.state = RecordingCaptureState::completed;
        incrementError();
        return false;
    }
    track.state = RecordingCaptureState::capturing;
    return true;
}

bool RecordingCaptureRuntime::endTrackCapture(const juce::String& trackId,
                                              const RecordingCaptureTrackState& track) noexcept {
    auto sink = acquireSink();
    if (!sink ||
        !sink->endAudioTrackCapture(trackId, track.endAudioSample, track.endTimelineSample)) {
        incrementError();
        return false;
    }
    return true;
}

bool RecordingCaptureRuntime::beginTailDrain(const juce::String& trackId,
                                             RecordingCaptureTrackState& track,
                                             const std::int64_t pluginDelaySamples,
                                             const std::int64_t pluginTailSamples) noexcept {
    if (track.state != RecordingCaptureState::capturing) return true;
    if (!endTrackCapture(trackId, track)) return false;

    const auto total = std::max<std::int64_t>(0, pluginDelaySamples + pluginTailSamples);
    track.tailRemainingSamples =
        static_cast<int>(std::min<std::int64_t>(total, std::numeric_limits<int>::max()));
    if (track.tailRemainingSamples == 0) {
        auto sink = acquireSink();
        if (!sink || !sink->completeAudioTrackTail(trackId)) {
            incrementError();
            return false;
        }
        track.state = RecordingCaptureState::idle;
        return true;
    }
    track.state = RecordingCaptureState::drainingTail;
    drainingTailTracksCount.fetch_add(1, std::memory_order_acq_rel);
    return true;
}

bool RecordingCaptureRuntime::drainTail(const juce::String& trackId,
                                        RecordingCaptureTrackState& track,
                                        juce::AudioBuffer<float>& silentInput,
                                        const int sampleCount) noexcept {
    if (track.state != RecordingCaptureState::drainingTail) return true;

    auto sink = acquireSink();
    if (!sink) {
        track.state = RecordingCaptureState::completed;
        incrementError();
        drainingTailTracksCount.fetch_sub(1, std::memory_order_acq_rel);
        return true;
    }

    const auto count = std::min({
        std::max(0, sampleCount),
        std::max(0, track.tailRemainingSamples),
        track.processedBuffer.getNumSamples(),
    });
    if (count <= 0) {
        track.state = RecordingCaptureState::completed;
        incrementError();
        drainingTailTracksCount.fetch_sub(1, std::memory_order_acq_rel);
        return true;
    }

    silentInput.clear(0, 0, count);
    silentInput.clear(1, 0, count);
    track.processedBuffer.clear(0, 0, count);
    track.processedBuffer.clear(1, 0, count);
    track.effectChain.process(silentInput.getArrayOfReadPointers(), 2,
                              track.processedBuffer.getArrayOfWritePointers(), 2, count);

    const auto discard = std::min(track.latencyToDiscard, count);
    track.latencyToDiscard -= discard;
    const auto processedCount = count - discard;
    if (processedCount > 0) {
        const std::array<const float*, 2> processed{
            track.processedBuffer.getReadPointer(0) + discard,
            track.processedBuffer.getReadPointer(1) + discard,
        };
        sink->writeAudioTrack(trackId, nullptr, 0, processed.data(), processedCount);
    }

    track.tailRemainingSamples -= count;
    if (track.tailRemainingSamples == 0) {
        if (!sink->completeAudioTrackTail(trackId)) incrementError();
        track.state = RecordingCaptureState::idle;
        drainingTailTracksCount.fetch_sub(1, std::memory_order_acq_rel);
    }
    return drainingTailTracksCount.load(std::memory_order_acquire) == 0;
}

void RecordingCaptureRuntime::writeAudioTrack(const juce::String& trackId, const float* const raw,
                                              const int rawSampleCount,
                                              const float* const* const processed,
                                              const int processedSampleCount) noexcept {
    auto sink = acquireSink();
    if (sink) sink->writeAudioTrack(trackId, raw, rawSampleCount, processed, processedSampleCount);
}

void RecordingCaptureRuntime::markLoopBoundary(const std::uint64_t audioSample) noexcept {
    auto sink = acquireSink();
    if (sink) sink->markLoopBoundary(audioSample);
}

void RecordingCaptureRuntime::writeMidiTrack(const juce::String& trackId,
                                             const juce::String& sourceDeviceId,
                                             const juce::MidiMessage& message,
                                             const std::uint64_t audioSample) noexcept {
    auto sink = acquireSink();
    if (sink) sink->writeMidiTrack(trackId, sourceDeviceId, message, audioSample);
}

void RecordingCaptureRuntime::setCaptureRange(const std::uint64_t startAudioSample,
                                              const std::uint64_t endAudioSample,
                                              const std::uint64_t startTimelineSample,
                                              const std::uint64_t endTimelineSample) noexcept {
    auto sink = acquireSink();
    if (sink)
        sink->setCaptureRange(startAudioSample, endAudioSample, startTimelineSample,
                              endTimelineSample);
}

void RecordingCaptureRuntime::resetCaptureErrors() noexcept {
    captureErrorCount.store(0, std::memory_order_release);
}

void RecordingCaptureRuntime::resetDrainingTailTracks() noexcept {
    drainingTailTracksCount.store(0, std::memory_order_release);
}

}  // namespace riffra
