#pragma once

#include <atomic>
#include <cstdint>

#include "ArrangementCaptureSink.h"
#include "PluginChain.h"

namespace riffra {

enum class RecordingCaptureState { idle, capturing, drainingTail, completed };

struct RecordingCaptureTrackState final {
    PluginChain effectChain;
    juce::AudioBuffer<float> processedBuffer;
    std::uint64_t endAudioSample = 0;
    std::uint64_t endTimelineSample = 0;
    int latencyToDiscard = 0;
    RecordingCaptureState state = RecordingCaptureState::idle;
    int tailRemainingSamples = 0;

    void reset() noexcept {
        effectChain.allNotesOff();
        endAudioSample = 0;
        endTimelineSample = 0;
        latencyToDiscard = 0;
        state = RecordingCaptureState::idle;
        tailRemainingSamples = 0;
    }
};

class RecordingCaptureRuntime final {
public:
    class SinkLease final {
    public:
        SinkLease() noexcept = default;
        SinkLease(const SinkLease&) = delete;
        SinkLease& operator=(const SinkLease&) = delete;
        SinkLease(SinkLease&& other) noexcept;
        SinkLease& operator=(SinkLease&& other) noexcept;
        ~SinkLease();

        [[nodiscard]] ArrangementCaptureSink* get() const noexcept { return sink; }
        [[nodiscard]] ArrangementCaptureSink* operator->() const noexcept { return sink; }
        [[nodiscard]] explicit operator bool() const noexcept { return sink != nullptr; }

    private:
        friend class RecordingCaptureRuntime;
        SinkLease(RecordingCaptureRuntime& owner, ArrangementCaptureSink* sink) noexcept;
        void release() noexcept;

        RecordingCaptureRuntime* owner = nullptr;
        ArrangementCaptureSink* sink = nullptr;
    };

    RecordingCaptureRuntime() = default;
    ~RecordingCaptureRuntime() = default;

    RecordingCaptureRuntime(const RecordingCaptureRuntime&) = delete;
    RecordingCaptureRuntime& operator=(const RecordingCaptureRuntime&) = delete;

    void setSink(ArrangementCaptureSink* sink) noexcept;
    void clearSink() noexcept;
    [[nodiscard]] SinkLease acquireSink() noexcept;

    void resetTrack(RecordingCaptureTrackState& track) noexcept;
    [[nodiscard]] bool hasCaptureWork(const RecordingCaptureTrackState& track) const noexcept;
    [[nodiscard]] bool beginTrackCapture(const juce::String& trackId,
                                         RecordingCaptureTrackState& track,
                                         std::uint64_t audioStartSample,
                                         std::uint64_t timelineStartSample) noexcept;
    [[nodiscard]] bool endTrackCapture(const juce::String& trackId,
                                       const RecordingCaptureTrackState& track) noexcept;
    [[nodiscard]] bool beginTailDrain(const juce::String& trackId,
                                      RecordingCaptureTrackState& track,
                                      std::int64_t pluginDelaySamples,
                                      std::int64_t pluginTailSamples) noexcept;
    [[nodiscard]] bool drainTail(const juce::String& trackId, RecordingCaptureTrackState& track,
                                 juce::AudioBuffer<float>& silentInput, int sampleCount) noexcept;

    void writeAudioTrack(const juce::String& trackId, const float* raw, int rawSampleCount,
                         const float* const* processed, int processedSampleCount) noexcept;
    void markLoopBoundary(std::uint64_t audioSample) noexcept;
    void writeMidiTrack(const juce::String& trackId, const juce::String& sourceDeviceId,
                        const juce::MidiMessage& message, std::uint64_t audioSample) noexcept;
    void setCaptureRange(std::uint64_t startAudioSample, std::uint64_t endAudioSample,
                         std::uint64_t startTimelineSample,
                         std::uint64_t endTimelineSample) noexcept;

    [[nodiscard]] unsigned int drainingTailTracks() const noexcept {
        return drainingTailTracksCount.load(std::memory_order_acquire);
    }
    [[nodiscard]] std::uint64_t captureErrors() const noexcept {
        return captureErrorCount.load(std::memory_order_acquire);
    }
    void resetCaptureErrors() noexcept;
    void resetDrainingTailTracks() noexcept;

private:
    void incrementError() noexcept { captureErrorCount.fetch_add(1, std::memory_order_relaxed); }

    std::atomic<ArrangementCaptureSink*> recordingSink{nullptr};
    std::atomic<unsigned int> recordingSinkReaders{0};
    std::atomic<unsigned int> drainingTailTracksCount{0};
    std::atomic<std::uint64_t> captureErrorCount{0};
};

}  // namespace riffra
