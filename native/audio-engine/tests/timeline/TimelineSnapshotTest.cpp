#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, RebuildsTimelineForTheCurrentAudioDeviceFormat) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::audioDeviceRestartRebuildsRuntimeFormat();

    // Assert
    EXPECT_TRUE(passed);
}

}  // namespace riffra
