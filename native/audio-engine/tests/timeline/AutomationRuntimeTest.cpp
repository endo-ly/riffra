#include <gtest/gtest.h>

#include "timeline/AutomationRuntime.h"
#include "timeline/MixAutomation.h"

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
    EXPECT_FLOAT_EQ(cursor.valueAt(-32, -1.0f), 0.0f);
    EXPECT_FLOAT_EQ(cursor.valueAt(320, -1.0f), 0.0f);
    lane.setPoints({{0, 0.0f}, {128, 1.0f}, {256, 0.0f}}, true);
    auto step = lane.cursorAt(64);
    EXPECT_FLOAT_EQ(step.valueAt(127, -1.0f), 0.0f);
    EXPECT_FLOAT_EQ(step.valueAt(128, -1.0f), 1.0f);
    EXPECT_FLOAT_EQ(step.valueAt(255, -1.0f), 1.0f);
    EXPECT_FLOAT_EQ(step.valueAt(256, -1.0f), 0.0f);
    EXPECT_FLOAT_EQ(step.valueAt(64, -1.0f), 0.0f);
    const auto segment = step.segmentAt(64, 256, -1.0f);
    EXPECT_EQ(segment.endSample, 128);
    EXPECT_FLOAT_EQ(segment.startValue, 0.0f);
    EXPECT_FLOAT_EQ(segment.endValue, 0.0f);
    AutomationRuntime gain, pan;
    gain.setPoints({{0, -24.0f}, {128, 0.0f}});
    pan.setPoints({{0, -1.0f}, {32, 0.0f}, {128, 0.0f}});
    MixAutomationCursor mix(gain, pan, 0, 129, 0.0f, 0.0f, true, true);
    MixAutomationCursor::Gains actual{};
    for (int sample = 0; sample <= 64; ++sample) actual = mix.next();
    EXPECT_NEAR(actual.left, juce::Decibels::decibelsToGain(-12.0f), 0.00001f);
    EXPECT_NEAR(actual.right, actual.left, 0.00001f);
}

}  // namespace
}  // namespace riffra
