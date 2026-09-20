#pragma once

#include <atomic>
#include <chrono>
#include <cstdint>

namespace riffra {

/// Thread-safe counters and peak meters produced by the realtime pipeline.
///
/// Peak values are consumed with exchange semantics so each projection period
/// observes only the audio processed since the previous read.
class AudioMetrics final {
public:
    struct TransientMeterSnapshot final {
        float inputPeak = 0.0f;
        float outputPeak = 0.0f;
        float outputPeakLeft = 0.0f;
        float outputPeakRight = 0.0f;
        float preLimiterPeak = 0.0f;
        float limiterGainReductionDb = 0.0f;
    };

    // Thread-safe. Writers run on the Audio thread and readers run on a
    // control or telemetry thread.
    [[nodiscard]] TransientMeterSnapshot consumeTransientMeters(
        std::uint64_t expectedProjectEpoch) const noexcept;
    [[nodiscard]] TransientMeterSnapshot peekTransientMeters(
        std::uint64_t expectedProjectEpoch) const noexcept;
    [[nodiscard]] std::uint64_t requestedProjectEpoch() const noexcept;
    [[nodiscard]] std::uint64_t invalidSampleCount() const noexcept;
    [[nodiscard]] std::uint64_t callbackCount() const noexcept;
    [[nodiscard]] std::uint64_t averageCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t maximumCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t callbackOverruns() const noexcept;
    [[nodiscard]] std::uint64_t hardClipSamples() const noexcept;

    // Audio thread only, except for the device reset called while the audio
    // boundary is controlled. Project epochs are requested by the graph
    // publication thread and applied at the beginning of an audio block.
    void requestProjectEpoch(std::uint64_t projectEpoch) noexcept;
    void beginProjectBlock(std::uint64_t projectEpoch) noexcept;
    void recordSilencedBlock(std::uint64_t projectEpoch, float inputPeak) noexcept;
    void recordBlock(std::uint64_t projectEpoch, float inputPeak, float preLimiterPeak,
                     float outputPeak, float outputPeakLeft, float outputPeakRight,
                     float limiterGainReductionDb, std::uint64_t hardClipSamples,
                     std::uint64_t invalidSamples) noexcept;
    void recordCallbackDuration(std::chrono::steady_clock::time_point started, int numSamples,
                                double sampleRate) noexcept;
    // Clears only values belonging to the current transient meter window.
    // Cumulative diagnostics remain available across graph boundaries.
    void resetTransientMeters() noexcept;
    void resetForDevice() noexcept;

private:
    [[nodiscard]] bool projectEpochIsCurrent(std::uint64_t projectEpoch) const noexcept;
    static void holdPeak(std::atomic<float>& peak, float value) noexcept;

    mutable std::atomic<float> inputPeakValue{0.0f};
    mutable std::atomic<float> outputPeakValue{0.0f};
    mutable std::atomic<float> outputPeakLeftValue{0.0f};
    mutable std::atomic<float> outputPeakRightValue{0.0f};
    std::atomic<std::uint64_t> requestedProjectEpochValue{0};
    std::atomic<std::uint64_t> activeProjectEpochValue{0};
    std::atomic<std::uint64_t> invalidSamples{0};
    std::atomic<std::uint64_t> callbackCountValue{0};
    std::atomic<std::uint64_t> callbackDurationUs{0};
    std::atomic<std::uint64_t> maximumCallbackDurationUsValue{0};
    std::atomic<std::uint64_t> callbackOverrunsValue{0};
    mutable std::atomic<float> preLimiterPeakValue{0.0f};
    mutable std::atomic<float> limiterGainReductionDbValue{0.0f};
    std::atomic<std::uint64_t> hardClipSamplesValue{0};
};

}  // namespace riffra
