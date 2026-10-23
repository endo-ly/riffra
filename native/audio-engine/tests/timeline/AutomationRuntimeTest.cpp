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

TEST(AutomationRuntimeTest, LinearAmplitudeTransitionAndBalanceRemainContinuousAcrossBlocks) {
    AutomationRuntime gain, pan;
    // 5 ms at 48 kHz: interpolate amplitude, retaining unity on the nearer side.
    gain.setPoints({{0, 1.0f}, {240, 0.5f}}, false, true);
    MixAutomationCursor first(gain, pan, 0, 120, 0.0f, 0.5f, false, true, {}, {}, true);
    auto initial = first.next();
    EXPECT_FLOAT_EQ(initial.left, 0.5f);
    EXPECT_FLOAT_EQ(initial.right, 1.0f);
    for (int sample = 1; sample < 120; ++sample) first.next();
    MixAutomationCursor second(gain, pan, 120, 121, 0.0f, 0.5f, false, true, {}, {}, true);
    auto middle = second.next();
    EXPECT_NEAR(middle.left, 0.375f, 1.0e-6f);
    EXPECT_NEAR(middle.right, 0.75f, 1.0e-6f);
    for (int sample = 121; sample < 240; ++sample) second.next();
    auto end = second.next();
    EXPECT_NEAR(end.left, 0.25f, 1.0e-6f);
    EXPECT_NEAR(end.right, 0.5f, 1.0e-6f);
}

}  // namespace
}  // namespace riffra
