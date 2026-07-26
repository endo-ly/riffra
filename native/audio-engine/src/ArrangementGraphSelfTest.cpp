#include <algorithm>
#include <array>

#include "ArrangementGraph.h"

namespace riffra {

juce::var runArrangementGraphSelfTest() {
    auto* result = new juce::DynamicObject();
    juce::Array<juce::var> checks;
    const auto add = [&checks](const juce::String& name, const bool passed) {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", name);
        check->setProperty("passed", passed);
        checks.add(juce::var(check));
    };

    add("MIDI device and channel routing is isolated",
        ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 2) &&
            !ArrangementGraph::midiRouteMatches("device-a", 2, "device-b", 2) &&
            !ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 3));

    std::array<float, 2> inputOne{0.25f, 0.5f};
    std::array<float, 2> inputTwo{-0.25f, -0.5f};
    const std::array<const float*, 2> physicalInputs{inputOne.data(), inputTwo.data()};
    add("Audio Tracks select independent physical input channels",
        ArrangementGraph::audioInputSource(0, physicalInputs.data(), 2) == inputOne.data() &&
            ArrangementGraph::audioInputSource(1, physicalInputs.data(), 2) == inputTwo.data() &&
            ArrangementGraph::audioInputSource(2, physicalInputs.data(), 2) == nullptr);

    add("Monitoring state is isolated per Audio Track",
        ArrangementGraph::shouldMonitorAudioInput("on", false, false) &&
            !ArrangementGraph::shouldMonitorAudioInput("off", true, false) &&
            ArrangementGraph::shouldMonitorAudioInput("auto", true, false) &&
            !ArrangementGraph::shouldMonitorAudioInput("auto", false, false) &&
            !ArrangementGraph::shouldMonitorAudioInput("on", true, true));

    add("PDC delays each Track to the maximum Chain latency",
        ArrangementGraph::compensationDelay(768, 256) == 512 &&
            ArrangementGraph::compensationDelay(768, 768) == 0 &&
            ArrangementGraph::compensationDelay(256, 768) == 0);

    const auto intersection = ArrangementGraph::captureIntersection(256, 256, 384, 256);
    add("capture taps use the exact Native Clock window",
        intersection.first == 384 && intersection.second == 512);

    const std::vector<ArrangementGraph::AutomationPoint> automation{
        {100, -12.0f},
        {200, 0.0f},
    };
    add("automation interpolates within an audio block",
        ArrangementGraph::automationValueAt(automation, 50, -6.0f) == -12.0f &&
            ArrangementGraph::automationValueAt(automation, 150, -6.0f) == -6.0f &&
            ArrangementGraph::automationValueAt(automation, 250, -6.0f) == 0.0f);

    const auto allPassed = std::all_of(checks.begin(), checks.end(), [](const juce::var& check) {
        return static_cast<bool>(check.getProperty("passed", false));
    });
    result->setProperty("type", "arrangementGraphSelfTest");
    result->setProperty("checks", checks);
    result->setProperty("passed", allPassed);
    return juce::var(result);
}

}  // namespace riffra
