#pragma once

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>

namespace riffra {

/// One-pole, one-zero DC blocking filter. Removes sub-sonic / DC offset from
/// the output path so the brick-wall limiter does not waste headroom on a
/// constant offset introduced by hardware or plugins. The coefficient 0.999
/// yields a cutoff well below 1 Hz at typical sample rates, leaving audio
/// content untouched.
class DCBlocker {
public:
    void prepare(const int numChannels) noexcept {
        activeChannels = std::min(numChannels, static_cast<int>(kMaxChannels));
        reset();
    }

    void reset() noexcept {
        prevInput.fill(0.0f);
        prevOutput.fill(0.0f);
    }

    void processBlock(float* const* data, const int numChannels, const int numSamples) noexcept {
        const auto channels = std::min(numChannels, activeChannels);
        for (int ch = 0; ch < channels; ++ch) {
            auto* channel = data[ch];
            if (channel == nullptr)
                continue;
            auto& pIn = prevInput[static_cast<std::size_t>(ch)];
            auto& pOut = prevOutput[static_cast<std::size_t>(ch)];
            for (int s = 0; s < numSamples; ++s) {
                const auto in = channel[s];
                const auto out = kCoefficient * (pOut + in - pIn);
                pIn = in;
                pOut = out;
                channel[s] = out;
            }
        }
    }

private:
    static constexpr int kMaxChannels = 32;
    static constexpr float kCoefficient = 0.999f;
    std::array<float, static_cast<std::size_t>(kMaxChannels)> prevInput {};
    std::array<float, static_cast<std::size_t>(kMaxChannels)> prevOutput {};
    int activeChannels = 0;
};

/// Detects sustained near-clipping input levels that indicate acoustic
/// feedback (howl) when software monitoring with a microphone and speakers.
/// The detector is intentionally conservative: it requires the input peak to
/// stay above 0.97 for at least kSustainedMs of contiguous samples before
/// flagging feedback, and resets the accumulator when the level drops below
/// 0.5 so legitimate loud passages do not trigger false positives.
class FeedbackDetector {
public:
    void prepare(const double sampleRate) noexcept {
        currentSampleRate = sampleRate > 0.0 ? sampleRate : 48000.0;
        reset();
    }

    void reset() noexcept {
        sustainedSamples = 0;
        feedbackSuspected.store(false, std::memory_order_relaxed);
    }

    void observe(const float peak, const int numSamples) noexcept {
        if (peak >= kPeakThreshold) {
            sustainedSamples += static_cast<std::uint64_t>(numSamples);
            const auto threshold = static_cast<std::uint64_t>(
                currentSampleRate * kSustainedMs / 1000.0);
            if (sustainedSamples >= threshold)
                feedbackSuspected.store(true, std::memory_order_release);
        } else if (peak < kReleaseThreshold) {
            sustainedSamples = 0;
        }
    }

    [[nodiscard]] bool isSuspected() const noexcept {
        return feedbackSuspected.load(std::memory_order_acquire);
    }

    bool consumeSuspected() noexcept {
        return feedbackSuspected.exchange(false, std::memory_order_acq_rel);
    }

private:
    static constexpr float kPeakThreshold = 0.97f;
    static constexpr float kReleaseThreshold = 0.5f;
    static constexpr double kSustainedMs = 250.0;
    double currentSampleRate = 48000.0;
    std::uint64_t sustainedSamples = 0;
    std::atomic<bool> feedbackSuspected { false };
};

} // namespace riffra
