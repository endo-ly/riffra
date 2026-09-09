#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, ProcessesTimelineAndLiveMidiInTheSameTrackContext) {
    EXPECT_TRUE(TimelineEngineTestPeer::timelineMidiUsesCurrentTransportContext());
}

}  // namespace riffra

