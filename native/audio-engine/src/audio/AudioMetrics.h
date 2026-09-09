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
    // Thread-safe. Writers run on the Audio thread and readers run on a
    // control or telemetry thread.
    [[nodiscard]] float inputPeak() const noexcept;
    [[nodiscard]] float outputPeak() const noexcept;
    [[nodiscard]] std::uint64_t invalidSampleCount() const noexcept;
    [[nodiscard]] std::uint64_t callbackCount() const noexcept;
    [[nodiscard]] std::uint64_t averageCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t maximumCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t callbackOverruns() const noexcept;
    [[nodiscard]] float preLimiterPeak() const noexcept;
    [[nodiscard]] float limiterGainReductionDb() const noexcept;
    [[nodiscard]] std::uint64_t hardClipSamples() const noexcept;

    // Audio thread only, except for resetForDevice which is called by the
    // device lifecycle thread before callbacks resume.
    void recordInputPeak(float peak) noexcept;
    void recordSilencedBlock(float inputPeak) noexcept;
    void recordBlock(float inputPeak, float preLimiterPeak, float outputPeak,
                     float limiterGainReductionDb, std::uint64_t hardClipSamples,
                     std::uint64_t invalidSamples) noexcept;
    void recordCallbackDuration(std::chrono::steady_clock::time_point started, int numSamples,
                                double sampleRate) noexcept;
    void resetForDevice() noexcept;

private:
    static void holdPeak(std::atomic<float>& peak, float value) noexcept;

    mutable std::atomic<float> inputPeakValue{0.0f};
    mutable std::atomic<float> outputPeakValue{0.0f};
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
