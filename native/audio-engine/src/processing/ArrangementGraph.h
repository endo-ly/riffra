#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <utility>

namespace riffra {

/// Realtime-safe routing and capture calculations shared by every Track node
/// in the Arrange graph. Stateful Track nodes remain owned by TimelineEngine;
/// this class is the policy boundary that keeps physical input, MIDI routing,
/// and capture taps independent from the master/playback buses.
class ArrangementGraph final {
public:
    [[nodiscard]] static bool midiRouteMatches(const juce::String& configuredDeviceId,
                                               int configuredChannel,
                                               const juce::String& sourceDeviceId,
                                               int messageChannel) noexcept;
    [[nodiscard]] static const float* audioInputSource(int configuredChannel,
                                                       const float* const* physicalInputChannels,
                                                       int physicalInputChannelCount) noexcept;
    [[nodiscard]] static bool shouldMonitorAudioInput(const juce::String& monitoring, bool armed,
                                                      bool instrument) noexcept;
    [[nodiscard]] static std::int64_t compensationDelay(std::int64_t maximumPluginDelay,
                                                        std::int64_t trackPluginDelay) noexcept;
    [[nodiscard]] static std::pair<int, int> captureIntersection(int chunkStart, int chunkSamples,
                                                                 int captureStart,
                                                                 int captureSamples) noexcept;
};

}  // namespace riffra
