#include <gtest/gtest.h>

#include <array>
#include <vector>

#include "timeline/ArrangementGraph.h"
#include "timeline/AutomationRuntime.h"
#include "timeline/MidiSourceRegistry.h"

namespace riffra {

TEST(ArrangementGraphTest, ResolvesMidiAndPhysicalInputRouting) {
    EXPECT_TRUE(ArrangementGraph::midiRouteMatches(0, 2, 0, 2));
    EXPECT_TRUE(ArrangementGraph::midiRouteMatches(MidiSourceRegistry::kAllSources, 2, 1, 2));
    EXPECT_TRUE(ArrangementGraph::midiRouteMatches(MidiSourceRegistry::kAllSources, 2,
                                                   MidiSourceRegistry::kUnregistered, 2));
    EXPECT_FALSE(ArrangementGraph::midiRouteMatches(0, 2, MidiSourceRegistry::kUnregistered, 2));
    EXPECT_FALSE(ArrangementGraph::midiRouteMatches(0, 2, 1, 2));
    EXPECT_FALSE(ArrangementGraph::midiRouteMatches(0, 2, 0, 3));

    std::array<float, 2> inputOne{0.25f, 0.5f};
    std::array<float, 2> inputTwo{-0.25f, -0.5f};
    const std::array<const float*, 2> physicalInputs{inputOne.data(), inputTwo.data()};

    EXPECT_EQ(ArrangementGraph::audioInputSource(0, physicalInputs.data(), 2), inputOne.data());
    EXPECT_EQ(ArrangementGraph::audioInputSource(1, physicalInputs.data(), 2), inputTwo.data());
    EXPECT_EQ(ArrangementGraph::audioInputSource(2, physicalInputs.data(), 2), nullptr);
}

TEST(ArrangementGraphTest, CalculatesPluginDelayCompensation) {
    EXPECT_EQ(ArrangementGraph::compensationDelay(768, 256), 512);
    EXPECT_EQ(ArrangementGraph::compensationDelay(768, 768), 0);
    EXPECT_EQ(ArrangementGraph::compensationDelay(256, 768), 0);
}

TEST(ArrangementGraphTest, IntersectsCaptureWithNativeClockWindow) {
    const auto intersection = ArrangementGraph::captureIntersection(256, 256, 384, 256);

    EXPECT_EQ(intersection.first, 384);
    EXPECT_EQ(intersection.second, 512);
}

TEST(ArrangementGraphTest, EvaluatesPreparedAutomationBlocks) {
    AutomationRuntime automationRuntime;
    automationRuntime.setPoints({
        {100, -12.0f},
        {200, 0.0f},
    });
    auto cursor = automationRuntime.cursorAt(50);

    EXPECT_FLOAT_EQ(cursor.valueAt(50, -6.0f), -12.0f);
    EXPECT_FLOAT_EQ(cursor.valueAt(150, -6.0f), -6.0f);
    EXPECT_FLOAT_EQ(cursor.valueAt(250, -6.0f), 0.0f);
}

}  // namespace riffra
