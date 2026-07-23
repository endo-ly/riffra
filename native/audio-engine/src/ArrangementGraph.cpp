#include "ArrangementGraph.h"

#include <algorithm>

namespace riffra {

bool ArrangementGraph::midiRouteMatches(
    const juce::String& configuredDeviceId,
    const int configuredChannel,
    const juce::String& sourceDeviceId,
    const int messageChannel) noexcept {
    return (configuredDeviceId.isEmpty() || configuredDeviceId == sourceDeviceId)
        && (configuredChannel == 0 || configuredChannel == messageChannel);
}

std::pair<int, int> ArrangementGraph::captureIntersection(
    const int chunkStart,
    const int chunkSamples,
    const int captureStart,
    const int captureSamples) noexcept {
    const auto start = std::max(chunkStart, captureStart);
    const auto end = std::min(
        chunkStart + std::max(0, chunkSamples),
        captureStart + std::max(0, captureSamples));
    return { start, std::max(start, end) };
}

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
        ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 2)
            && !ArrangementGraph::midiRouteMatches("device-a", 2, "device-b", 2)
            && !ArrangementGraph::midiRouteMatches("device-a", 2, "device-a", 3));
    const auto intersection = ArrangementGraph::captureIntersection(256, 256, 384, 256);
    add("capture taps use the exact Native Clock window",
        intersection.first == 384 && intersection.second == 512);
    result->setProperty("type", "arrangementGraphSelfTest");
    result->setProperty("checks", checks);
    result->setProperty("passed",
        checks.size() == 2
            && static_cast<bool>(checks[0].getProperty("passed", false))
            && static_cast<bool>(checks[1].getProperty("passed", false)));
    return juce::var(result);
}

} // namespace riffra
