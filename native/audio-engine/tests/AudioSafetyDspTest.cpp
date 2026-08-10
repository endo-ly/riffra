#include "AudioSafetyDsp.h"

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>

namespace riffra {
namespace {

constexpr double kSampleRate = 48000.0;
constexpr int kBlockSize = 256;
constexpr int kNumChannels = 2;

} // namespace

TEST(AudioSafetyDspTest, DCBlockerRemovesConstantOffset)
{
    DCBlocker blocker;
    blocker.prepare(kNumChannels);

    std::array<std::array<float, kBlockSize>, kNumChannels> buffers {};
    std::array<float*, kNumChannels> channelPointers {};
    for (int channel = 0; channel < kNumChannels; ++channel)
        channelPointers[static_cast<std::size_t>(channel)] = buffers[channel].data();

    float lastSample = 0.0f;
    const auto blocks = static_cast<int>(kSampleRate * 0.5 / kBlockSize);
    for (int block = 0; block < blocks; ++block) {
        for (auto& buffer : buffers)
            buffer.fill(0.5f);

        blocker.processBlock(channelPointers.data(), kNumChannels, kBlockSize);
        lastSample = buffers[0].back();
    }

    EXPECT_LT(std::abs(lastSample), 0.01f);
}

TEST(AudioSafetyDspTest, DCBlockerPreservesAudioSignal)
{
    DCBlocker blocker;
    blocker.prepare(kNumChannels);

    std::array<std::array<float, kBlockSize>, kNumChannels> buffers {};
    std::array<float*, kNumChannels> channelPointers {};
    for (int channel = 0; channel < kNumChannels; ++channel)
        channelPointers[static_cast<std::size_t>(channel)] = buffers[channel].data();

    constexpr float twoPi = 6.2831853071795864769f;
    constexpr float frequency = 440.0f;
    constexpr float amplitude = 0.5f;
    float phase = 0.0f;
    const auto phaseStep = twoPi * frequency / static_cast<float>(kSampleRate);
    float maximumAmplitude = 0.0f;
    const auto blocks = static_cast<int>(kSampleRate * 0.5 / kBlockSize);

    for (int block = 0; block < blocks; ++block) {
        for (int sample = 0; sample < kBlockSize; ++sample) {
            buffers[0][sample] = std::sin(phase) * amplitude;
            buffers[1][sample] = buffers[0][sample];
            phase += phaseStep;
            if (phase >= twoPi)
                phase -= twoPi;
        }

        blocker.processBlock(channelPointers.data(), kNumChannels, kBlockSize);
        for (int sample = kBlockSize / 2; sample < kBlockSize; ++sample)
            maximumAmplitude = std::max(maximumAmplitude, std::abs(buffers[0][sample]));
    }

    EXPECT_GT(maximumAmplitude, 0.3f);
    EXPECT_LT(maximumAmplitude, 0.6f);
}

TEST(AudioSafetyDspTest, FeedbackDetectorDetectsSustainedNearPeakInput)
{
    FeedbackDetector detector;
    detector.prepare(kSampleRate);

    const auto sustainedBlocks = static_cast<int>(
        std::ceil(kSampleRate * 300.0 / 1000.0 / kBlockSize));
    for (int block = 0; block < sustainedBlocks; ++block)
        detector.observe(0.99f, kBlockSize, true);

    EXPECT_TRUE(detector.isSuspected());
}

TEST(AudioSafetyDspTest, FeedbackDetectorIgnoresBriefPeak)
{
    FeedbackDetector detector;
    detector.prepare(kSampleRate);

    const auto briefBlocks = static_cast<int>(
        std::ceil(kSampleRate * 50.0 / 1000.0 / kBlockSize));
    for (int block = 0; block < briefBlocks; ++block)
        detector.observe(0.99f, kBlockSize, true);
    detector.observe(0.1f, kBlockSize, true);

    EXPECT_FALSE(detector.isSuspected());
}

TEST(AudioSafetyDspTest, FeedbackDetectorIgnoresPeakInputWhenMonitoringIsOff)
{
    FeedbackDetector detector;
    detector.prepare(kSampleRate);

    const auto sustainedBlocks = static_cast<int>(
        std::ceil(kSampleRate * 300.0 / 1000.0 / kBlockSize));
    for (int block = 0; block < sustainedBlocks; ++block)
        detector.observe(0.99f, kBlockSize, false);

    EXPECT_FALSE(detector.isSuspected());
}

} // namespace riffra
