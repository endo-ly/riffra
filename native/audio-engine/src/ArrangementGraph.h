#pragma once

#include <JuceHeader.h>
#include <cstdint>
#include <utility>
#include <vector>

namespace riffra {

/// Realtime-safe routing and capture calculations shared by every Track node
/// in the Arrange graph. Stateful Track nodes remain owned by TimelineEngine;
/// this class is the policy boundary that keeps physical input, MIDI routing,
/// and capture taps independent from the master/playback buses.
class ArrangementGraph final {
public:
    struct AutomationPoint final {
        std::int64_t sample = 0;
        float value = 0.0f;
    };

    [[nodiscard]] static bool midiRouteMatches(
        const juce::String& configuredDeviceId,
        int configuredChannel,
        const juce::String& sourceDeviceId,
        int messageChannel) noexcept;
    [[nodiscard]] static const float* audioInputSource(
        int configuredChannel,
        const float* const* physicalInputChannels,
        int physicalInputChannelCount) noexcept;
    [[nodiscard]] static bool shouldMonitorAudioInput(
        const juce::String& monitoring,
        bool armed,
        bool instrument) noexcept;
    [[nodiscard]] static std::int64_t compensationDelay(
        std::int64_t maximumPluginDelay,
        std::int64_t trackPluginDelay) noexcept;
    [[nodiscard]] static std::pair<int, int> captureIntersection(
        int chunkStart,
        int chunkSamples,
        int captureStart,
        int captureSamples) noexcept;
    [[nodiscard]] static float automationValueAt(
        const std::vector<AutomationPoint>& points,
        std::int64_t sample,
        float fallback) noexcept;
};

[[nodiscard]] juce::var runArrangementGraphSelfTest();

} // namespace riffra
