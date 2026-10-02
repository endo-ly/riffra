#pragma once

#include <atomic>
#include <cstdint>

#include "ArrangementCaptureSink.h"
#include "concurrency/RetireQueue.h"
namespace riffra {

enum class RecordingCaptureState { idle, capturing };

struct RecordingCaptureTrackState final {
    std::uint64_t endAudioSample = 0;
    std::uint64_t endTimelineSample = 0;
    RecordingCaptureState state = RecordingCaptureState::idle;

    void reset() noexcept {
        endAudioSample = 0;
        endTimelineSample = 0;
        state = RecordingCaptureState::idle;
    }
};

class RecordingCaptureRuntime final {
public:
    RecordingCaptureRuntime() = default;
    ~RecordingCaptureRuntime() = default;

    RecordingCaptureRuntime(const RecordingCaptureRuntime&) = delete;
    RecordingCaptureRuntime& operator=(const RecordingCaptureRuntime&) = delete;

    void setSink(ArrangementCaptureSink* sink) noexcept;
    void clearSink() noexcept;
    /// Control side only, after a clear command has been submitted.
    bool waitForRetiredSink(ArrangementCaptureSink* sink) noexcept;

    void resetTrack(RecordingCaptureTrackState& track) noexcept;
    [[nodiscard]] bool hasCaptureWork(const RecordingCaptureTrackState& track) const noexcept;
    [[nodiscard]] bool beginTrackCapture(const juce::String& trackId,
                                         RecordingCaptureTrackState& track,
                                         std::uint64_t audioStartSample,
                                         std::uint64_t timelineStartSample) noexcept;
    [[nodiscard]] bool endTrackCapture(const juce::String& trackId,
                                       const RecordingCaptureTrackState& track) noexcept;
    void writeAudioTrack(const juce::String& trackId, const float* raw,
                         int rawSampleCount) noexcept;
    void markLoopBoundary(std::uint64_t audioSample) noexcept;
    void writeMidiTrack(const juce::String& trackId, const juce::String& sourceDeviceId,
                        const juce::MidiMessage& message, std::uint64_t audioSample) noexcept;
    void writeMidiTrack(const juce::String& trackId, std::uint16_t sourceIndex,
                        const juce::MidiMessage& message, std::uint64_t audioSample) noexcept;
    void setCaptureRange(std::uint64_t startAudioSample, std::uint64_t endAudioSample,
                         std::uint64_t startTimelineSample,
                         std::uint64_t endTimelineSample) noexcept;

    [[nodiscard]] std::uint64_t captureErrors() const noexcept {
        return captureErrorCount.load(std::memory_order_acquire);
    }
    void resetCaptureErrors() noexcept;

private:
    void incrementError() noexcept { captureErrorCount.fetch_add(1, std::memory_order_relaxed); }

    ArrangementCaptureSink* sink = nullptr;
    RetireQueue<ArrangementCaptureSink, 256> retiredSinks;
    std::atomic<std::uint64_t> captureErrorCount{0};
};

}  // namespace riffra
