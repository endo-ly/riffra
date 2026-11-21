#include <gtest/gtest.h>

#include "device/AudioDeviceService.h"

namespace riffra {

TEST(AudioDeviceServiceTest, ReportsProbeFieldsRequiredByTheHost) {
    const auto probe = encodeAudioDeviceProbe(AudioDeviceService::discover());

    ASSERT_TRUE(probe.isObject());
    EXPECT_EQ(probe.getProperty("type", {}).toString(), "audioDeviceProbe");
    EXPECT_TRUE(static_cast<bool>(probe.getProperty("drivers", {}).isArray()));
    EXPECT_GT(static_cast<juce::int64>(probe.getProperty("refreshedAtMs", 0)), 0);
    EXPECT_EQ(probe.getProperty("message", {}).toString(), "Audio device list refreshed.");
}

}  // namespace riffra
