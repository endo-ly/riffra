#include <gtest/gtest.h>

#include <vector>

#include "audio/AudioMetrics.h"

namespace riffra {

class AudioMetricsTestPeer final {
public:
    static bool record(AudioMetrics& metrics, std::uint64_t durationUs, int samples,
                       double sampleRate = 1000.0) {
        const auto closed = metrics.beginCallback(samples, sampleRate);
        return metrics.recordCallbackDurationUs(durationUs, samples, sampleRate) || closed;
    }
};

TEST(AudioMetricsTest, PublishesOnlyTheCallbacksInEachAudioClockWindow) {
    // Arrange
    AudioMetrics metrics;

    // Act
    EXPECT_FALSE(AudioMetricsTestPeer::record(metrics, 600'000, 500));
    EXPECT_TRUE(AudioMetricsTestPeer::record(metrics, 100'000, 500));
    const auto first = metrics.callbackWindow();
    EXPECT_FALSE(AudioMetricsTestPeer::record(metrics, 20, 250));
    const auto retained = metrics.callbackWindow();
    EXPECT_TRUE(AudioMetricsTestPeer::record(metrics, 60, 750));
    const auto second = metrics.callbackWindow();

    // Assert
    EXPECT_EQ(first.windowEndAudioSample, 1000u);
    EXPECT_EQ(first.callbackCount, 2u);
    EXPECT_EQ(first.overruns, 1u);
    EXPECT_EQ(first.averageCallbackDurationUs, 350'000u);
    EXPECT_EQ(first.maximumCallbackDurationUs, 600'000u);
    EXPECT_EQ(retained.windowEndAudioSample, first.windowEndAudioSample);
    EXPECT_EQ(second.windowEndAudioSample, 2000u);
    EXPECT_EQ(second.callbackCount, 2u);
    EXPECT_EQ(second.overruns, 0u);
    EXPECT_EQ(second.averageCallbackDurationUs, 40u);
    EXPECT_EQ(second.maximumCallbackDurationUs, 60u);
    EXPECT_EQ(metrics.callbackCount(), 4u);
    EXPECT_EQ(metrics.callbackOverruns(), 1u);
}

TEST(AudioMetricsTest, AssignsNonDivisibleBlocksToFixedWindowEndSamples) {
    // Arrange
    AudioMetrics metrics;
    std::vector<CallbackWindow> windows;

    // Act
    for (int callback = 1; callback <= 375; ++callback) {
        const auto duration = callback == 94 ? 11'000 : callback == 188 ? 12'000 : 100;
        if (AudioMetricsTestPeer::record(metrics, duration, 512, 48'000.0))
            windows.push_back(metrics.callbackWindow());
    }

    // Assert
    ASSERT_EQ(windows.size(), 4u);
    std::uint32_t callbacks = 0;
    for (std::size_t index = 0; index < windows.size(); ++index) {
        EXPECT_EQ(windows[index].windowEndAudioSample, (index + 1) * 48'000u);
        EXPECT_EQ(windows[index].callbackCount, index == 0 ? 93u : 94u);
        callbacks += windows[index].callbackCount;
    }
    EXPECT_EQ(callbacks, 375u);
    EXPECT_EQ(windows[0].maximumCallbackDurationUs, 100u);
    EXPECT_EQ(windows[1].averageCallbackDurationUs, (93u * 100u + 11'000u) / 94u);
    EXPECT_EQ(windows[1].maximumCallbackDurationUs, 11'000u);
    EXPECT_EQ(windows[1].overruns, 1u);
    EXPECT_EQ(windows[2].maximumCallbackDurationUs, 12'000u);
    EXPECT_EQ(windows[2].overruns, 1u);
    EXPECT_EQ(windows[3].overruns, 0u);
    EXPECT_EQ(metrics.callbackCount(), 375u);
    EXPECT_EQ(metrics.callbackOverruns(), 2u);
}

}  // namespace riffra
