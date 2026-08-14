#include <gtest/gtest.h>

#include <array>
#include <vector>

#include "ArrangementGraph.h"

namespace riffra {

TEST(ArrangementGraphTest, MidiRouteMatchesDeviceAndChannel) {
    EXPECT_TRUE(ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 2));
    EXPECT_FALSE(ArrangementGraph::midiRouteMatches("device-a", 2, "device-b", 2));
    EXPECT_FALSE(ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 3));
}

TEST(ArrangementGraphTest, SelectsRequestedPhysicalInputChannel) {
    std::array<float, 2> inputOne{0.25f, 0.5f};
    std::array<float, 2> inputTwo{-0.25f, -0.5f};
    const std::array<const float*, 2> physicalInputs{inputOne.data(), inputTwo.data()};

    EXPECT_EQ(ArrangementGraph::audioInputSource(0, physicalInputs.data(), 2), inputOne.data());
    EXPECT_EQ(ArrangementGraph::audioInputSource(1, physicalInputs.data(), 2), inputTwo.data());
    EXPECT_EQ(ArrangementGraph::audioInputSource(2, physicalInputs.data(), 2), nullptr);
}

TEST(ArrangementGraphTest, DeterminesMonitoringFromModeAndRecordingState) {
    EXPECT_TRUE(ArrangementGraph::shouldMonitorAudioInput("on", false, false));
    EXPECT_FALSE(ArrangementGraph::shouldMonitorAudioInput("off", true, false));
    EXPECT_TRUE(ArrangementGraph::shouldMonitorAudioInput("auto", true, false));
    EXPECT_FALSE(ArrangementGraph::shouldMonitorAudioInput("auto", false, false));
    EXPECT_FALSE(ArrangementGraph::shouldMonitorAudioInput("on", true, true));
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

TEST(ArrangementGraphTest, InterpolatesAutomationValues) {
    const std::vector<ArrangementGraph::AutomationPoint> automation{
        {100, -12.0f},
        {200, 0.0f},
    };

    EXPECT_FLOAT_EQ(ArrangementGraph::automationValueAt(automation, 50, -6.0f), -12.0f);
    EXPECT_FLOAT_EQ(ArrangementGraph::automationValueAt(automation, 150, -6.0f), -6.0f);
    EXPECT_FLOAT_EQ(ArrangementGraph::automationValueAt(automation, 250, -6.0f), 0.0f);
}

}  // namespace riffra
