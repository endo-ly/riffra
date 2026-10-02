#include "RecordingCaptureRuntime.h"

#include <chrono>
#include <thread>
#include <utility>

namespace riffra {

void RecordingCaptureRuntime::setSink(ArrangementCaptureSink* const next) noexcept {
    auto* previous = std::exchange(sink, next);
    if (previous != nullptr && !retiredSinks.retire(previous)) jassertfalse;
}

void RecordingCaptureRuntime::clearSink() noexcept { setSink(nullptr); }

bool RecordingCaptureRuntime::waitForRetiredSink(ArrangementCaptureSink* const expected) noexcept {
    if (expected == nullptr) return true;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(1);
    bool returned = false;
    do {
        retiredSinks.reclaim([&](ArrangementCaptureSink* retired) {
            if (retired == expected) returned = true;
        });
        if (returned) return true;
        if (std::chrono::steady_clock::now() >= deadline) return false;
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    } while (true);
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
    if (!sink ||
        !sink->endAudioTrackCapture(trackId, track.endAudioSample, track.endTimelineSample)) {
        incrementError();
        return false;
    }
    return true;
}

void RecordingCaptureRuntime::writeAudioTrack(const juce::String& trackId, const float* const raw,
                                              const int rawSampleCount) noexcept {
    if (sink) sink->writeAudioTrack(trackId, raw, rawSampleCount);
}

void RecordingCaptureRuntime::markLoopBoundary(const std::uint64_t audioSample) noexcept {
    if (sink) sink->markLoopBoundary(audioSample);
}

void RecordingCaptureRuntime::writeMidiTrack(const juce::String& trackId,
                                             const juce::String& sourceDeviceId,
                                             const juce::MidiMessage& message,
                                             const std::uint64_t audioSample) noexcept {
    if (sink) sink->writeMidiTrack(trackId, sourceDeviceId, message, audioSample);
}

void RecordingCaptureRuntime::writeMidiTrack(const juce::String& trackId,
                                             const std::uint16_t sourceIndex,
                                             const juce::MidiMessage& message,
                                             const std::uint64_t audioSample) noexcept {
    if (sink) sink->writeMidiTrack(trackId, sourceIndex, message, audioSample);
}

void RecordingCaptureRuntime::setCaptureRange(const std::uint64_t startAudioSample,
                                              const std::uint64_t endAudioSample,
                                              const std::uint64_t startTimelineSample,
                                              const std::uint64_t endTimelineSample) noexcept {
    if (sink)
        sink->setCaptureRange(startAudioSample, endAudioSample, startTimelineSample,
                              endTimelineSample);
}

void RecordingCaptureRuntime::resetCaptureErrors() noexcept {
    captureErrorCount.store(0, std::memory_order_release);
}

}  // namespace riffra
