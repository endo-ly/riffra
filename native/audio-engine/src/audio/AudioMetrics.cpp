#include "AudioMetrics.h"

#include <algorithm>

namespace riffra {

AudioMetrics::TransientMeterSnapshot AudioMetrics::consumeTransientMeters(
    const std::uint64_t expectedProjectEpoch) const noexcept {
    if (!projectEpochIsCurrent(expectedProjectEpoch)) return {};
    return {
        inputPeakValue.exchange(0.0f, std::memory_order_acq_rel),
        outputPeakValue.exchange(0.0f, std::memory_order_acq_rel),
        outputPeakLeftValue.exchange(0.0f, std::memory_order_acq_rel),
        outputPeakRightValue.exchange(0.0f, std::memory_order_acq_rel),
        preLimiterPeakValue.exchange(0.0f, std::memory_order_acq_rel),
        limiterGainReductionDbValue.exchange(0.0f, std::memory_order_acq_rel),
    };
}

AudioMetrics::TransientMeterSnapshot AudioMetrics::peekTransientMeters(
    const std::uint64_t expectedProjectEpoch) const noexcept {
    if (!projectEpochIsCurrent(expectedProjectEpoch)) return {};
    return {
        inputPeakValue.load(std::memory_order_acquire),
        outputPeakValue.load(std::memory_order_acquire),
        outputPeakLeftValue.load(std::memory_order_acquire),
        outputPeakRightValue.load(std::memory_order_acquire),
        preLimiterPeakValue.load(std::memory_order_acquire),
        limiterGainReductionDbValue.load(std::memory_order_acquire),
    };
}

std::uint64_t AudioMetrics::requestedProjectEpoch() const noexcept {
    return requestedProjectEpochValue.load(std::memory_order_acquire);
}

std::uint64_t AudioMetrics::invalidSampleCount() const noexcept {
    return invalidSamples.load(std::memory_order_acquire);
}

std::uint64_t AudioMetrics::callbackCount() const noexcept {
    return callbackCountValue.load(std::memory_order_acquire);
}

std::uint64_t AudioMetrics::averageCallbackDurationUs() const noexcept {
    const auto count = callbackCount();
    return count == 0 ? 0 : callbackDurationUs.load(std::memory_order_acquire) / count;
}

std::uint64_t AudioMetrics::maximumCallbackDurationUs() const noexcept {
    return maximumCallbackDurationUsValue.load(std::memory_order_acquire);
}

std::uint64_t AudioMetrics::callbackOverruns() const noexcept {
    return callbackOverrunsValue.load(std::memory_order_acquire);
}

std::uint64_t AudioMetrics::hardClipSamples() const noexcept {
    return hardClipSamplesValue.load(std::memory_order_acquire);
}

bool AudioMetrics::projectEpochIsCurrent(const std::uint64_t projectEpoch) const noexcept {
    return requestedProjectEpochValue.load(std::memory_order_acquire) == projectEpoch &&
           activeProjectEpochValue.load(std::memory_order_acquire) == projectEpoch;
}

void AudioMetrics::holdPeak(std::atomic<float>& peak, const float value) noexcept {
    auto current = peak.load(std::memory_order_relaxed);
    while (value > current && !peak.compare_exchange_weak(current, value, std::memory_order_release,
                                                          std::memory_order_relaxed)) {
    }
}

void AudioMetrics::requestProjectEpoch(const std::uint64_t projectEpoch) noexcept {
    requestedProjectEpochValue.store(projectEpoch, std::memory_order_release);
}

void AudioMetrics::beginProjectBlock(const std::uint64_t projectEpoch) noexcept {
    if (activeProjectEpochValue.load(std::memory_order_acquire) == projectEpoch) return;
    resetTransientMeters();
    activeProjectEpochValue.store(projectEpoch, std::memory_order_release);
}

void AudioMetrics::recordSilencedBlock(const std::uint64_t projectEpoch,
                                       const float peak) noexcept {
    if (!projectEpochIsCurrent(projectEpoch)) return;
    holdPeak(inputPeakValue, peak);
    outputPeakValue.store(0.0f, std::memory_order_release);
    outputPeakLeftValue.store(0.0f, std::memory_order_release);
    outputPeakRightValue.store(0.0f, std::memory_order_release);
}

void AudioMetrics::recordBlock(const std::uint64_t projectEpoch, const float blockInputPeak,
                               const float blockPreLimiterPeak, const float blockOutputPeak,
                               const float blockOutputPeakLeft, const float blockOutputPeakRight,
                               const float blockLimiterGainReductionDb,
                               const std::uint64_t blockHardClipSamples,
                               const std::uint64_t blockInvalidSamples) noexcept {
    if (!projectEpochIsCurrent(projectEpoch)) return;
    holdPeak(inputPeakValue, blockInputPeak);
    holdPeak(preLimiterPeakValue, blockPreLimiterPeak);
    holdPeak(outputPeakValue, blockOutputPeak);
    holdPeak(outputPeakLeftValue, blockOutputPeakLeft);
    holdPeak(outputPeakRightValue, blockOutputPeakRight);
    if (blockLimiterGainReductionDb > 0.0f)
        holdPeak(limiterGainReductionDbValue, blockLimiterGainReductionDb);
    if (blockHardClipSamples > 0)
        hardClipSamplesValue.fetch_add(blockHardClipSamples, std::memory_order_relaxed);
    if (blockInvalidSamples > 0)
        invalidSamples.fetch_add(blockInvalidSamples, std::memory_order_relaxed);
}

void AudioMetrics::recordCallbackDuration(const std::chrono::steady_clock::time_point started,
                                          const int numSamples, const double sampleRate) noexcept {
    const auto duration = std::chrono::duration_cast<std::chrono::microseconds>(
                              std::chrono::steady_clock::now() - started)
                              .count();
    const auto durationUs = static_cast<std::uint64_t>(std::max<std::int64_t>(0, duration));
    callbackCountValue.fetch_add(1, std::memory_order_relaxed);
    callbackDurationUs.fetch_add(durationUs, std::memory_order_relaxed);
    auto maximum = maximumCallbackDurationUsValue.load(std::memory_order_relaxed);
    while (durationUs > maximum &&
           !maximumCallbackDurationUsValue.compare_exchange_weak(
               maximum, durationUs, std::memory_order_release, std::memory_order_relaxed)) {
    }
    if (sampleRate > 0.0 && numSamples > 0 &&
        static_cast<double>(durationUs) >
            1'000'000.0 * static_cast<double>(numSamples) / sampleRate)
        callbackOverrunsValue.fetch_add(1, std::memory_order_relaxed);
}

void AudioMetrics::resetTransientMeters() noexcept {
    inputPeakValue.store(0.0f, std::memory_order_release);
    outputPeakValue.store(0.0f, std::memory_order_release);
    outputPeakLeftValue.store(0.0f, std::memory_order_release);
    outputPeakRightValue.store(0.0f, std::memory_order_release);
    preLimiterPeakValue.store(0.0f, std::memory_order_release);
    limiterGainReductionDbValue.store(0.0f, std::memory_order_release);
}

void AudioMetrics::resetForDevice() noexcept {
    resetTransientMeters();
    hardClipSamplesValue.store(0, std::memory_order_release);
}

}  // namespace riffra
