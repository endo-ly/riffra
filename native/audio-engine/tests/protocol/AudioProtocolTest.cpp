#include <gtest/gtest.h>

#include <string>

#include "protocol/OutputQueue.h"

namespace riffra {

TEST(AudioProtocolTest, ControlOutputDiscardsQueuedTelemetry) {
    OutputQueue queue;

    queue.enqueueTelemetry("old telemetry");
    queue.enqueueControl("new control");
    ASSERT_TRUE(queue.hasControl());
    EXPECT_EQ(queue.takeControl(), "new control");
    EXPECT_TRUE(queue.empty());
}

TEST(AudioProtocolTest, ClosingStateSurvivesSaturationAndFollowsEarlierState) {
    OutputQueue queue;
    for (std::size_t index = 0; index < OutputQueue::kStateQueueLimit; ++index)
        queue.enqueueState(std::to_string(index), std::to_string(index));
    queue.enqueueState("overflow", "dropped");
    queue.enqueueClosingState("final");

    for (std::size_t index = 0; index < OutputQueue::kStateQueueLimit; ++index) {
        ASSERT_TRUE(queue.hasControl());
        EXPECT_EQ(queue.takeControl(), std::to_string(index));
    }
    ASSERT_TRUE(queue.hasControl());
    EXPECT_EQ(queue.takeControl(), "final");
    EXPECT_TRUE(queue.empty());
}

}  // namespace riffra
