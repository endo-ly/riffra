#include <gtest/gtest.h>

#include "support/TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, ProcessesTimelineAndLiveMidiInTheSameTrackContext) {
    EXPECT_TRUE(TimelineEngineTestPeer::timelineMidiUsesCurrentTransportContext());
}

}  // namespace riffra
