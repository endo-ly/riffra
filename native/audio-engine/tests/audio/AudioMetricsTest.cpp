#include <gtest/gtest.h>

#include "audio/AudioMetrics.h"

namespace riffra {

class AudioMetricsTestPeer final {
public:
    static bool record(AudioMetrics& metrics, std::uint64_t durationUs, int samples) {
        return metrics.recordCallbackDurationUs(durationUs, samples, 1000.0);
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

}  // namespace riffra
