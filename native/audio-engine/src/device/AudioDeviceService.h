#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <memory>
#include <optional>

#include "AudioConfiguration.h"

namespace riffra {

class AudioDeviceService final {
public:
    // Control thread only. This service discovers devices and prepares setup
    // data; it does not own the live device lifecycle.
    [[nodiscard]] static juce::var discover();
    [[nodiscard]] static std::optional<juce::var> probeDeviceChannels(
        const juce::String& driver, const juce::String& inputDevice,
        const juce::String& outputDevice, juce::String& error);
    [[nodiscard]] static juce::String initialise(juce::AudioDeviceManager& manager,
                                                 const AudioConfiguration& configuration);

private:
    [[nodiscard]] static juce::String accessModeForDriver(const juce::String& driver);
    [[nodiscard]] static bool driverRequiresSameDevice(const juce::String& driver);
    [[nodiscard]] static juce::String defaultDriver();
};

}  // namespace riffra
