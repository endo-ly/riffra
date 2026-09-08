#include "RecordingCaptureRuntime.h"

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
    return track.state == RecordingCaptureState::capturing;
}

bool RecordingCaptureRuntime::beginTrackCapture(const juce::String& trackId,
                                                RecordingCaptureTrackState& track,
                                                const std::uint64_t audioStartSample,
                                                const std::uint64_t timelineStartSample) noexcept {
    auto sink = acquireSink();
    if (!sink || !sink->beginAudioTrackCapture(trackId, audioStartSample, timelineStartSample)) {
        track.state = RecordingCaptureState::idle;
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

void RecordingCaptureRuntime::writeAudioTrack(const juce::String& trackId, const float* const raw,
                                              const int rawSampleCount) noexcept {
    auto sink = acquireSink();
    if (sink) sink->writeAudioTrack(trackId, raw, rawSampleCount);
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

}  // namespace riffra
