#include <gtest/gtest.h>

#include "AutomationRuntime.h"

namespace riffra {
namespace {

TEST(AutomationRuntimeTest, CursorPreservesPointsInsideAnAudioBlock) {
    AutomationRuntime lane;
    lane.setPoints({{0, 0.0f}, {128, 1.0f}, {256, 0.0f}});
    auto cursor = lane.cursorAt(0);

    EXPECT_FLOAT_EQ(cursor.valueAt(0, -1.0f), 0.0f);
    EXPECT_FLOAT_EQ(cursor.valueAt(64, -1.0f), 0.5f);
    EXPECT_FLOAT_EQ(cursor.valueAt(128, -1.0f), 1.0f);
    EXPECT_FLOAT_EQ(cursor.valueAt(192, -1.0f), 0.5f);
    EXPECT_FLOAT_EQ(cursor.valueAt(256, -1.0f), 0.0f);
}

}  // namespace
}  // namespace riffra
