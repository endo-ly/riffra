#pragma once

#include <JuceHeader.h>
#include <utility>

namespace riffra {

/// Realtime-safe routing and capture calculations shared by every Track node
/// in the Arrange graph. Stateful Track nodes remain owned by TimelineEngine;
/// this class is the policy boundary that keeps physical input, MIDI routing,
/// and capture taps independent from the master/playback buses.
class ArrangementGraph final {
public:
    [[nodiscard]] static bool midiRouteMatches(
        const juce::String& configuredDeviceId,
        int configuredChannel,
        const juce::String& sourceDeviceId,
        int messageChannel) noexcept;
    [[nodiscard]] static std::pair<int, int> captureIntersection(
        int chunkStart,
        int chunkSamples,
        int captureStart,
        int captureSamples) noexcept;
};

[[nodiscard]] juce::var runArrangementGraphSelfTest();

} // namespace riffra
