#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <memory>
#include <optional>

namespace riffra {

class MidiMonitor;
class SafetyAudioCallback;
class TimelineEngine;

struct AudioConfiguration {
    juce::String driver;
    juce::String inputDevice;
    juce::String outputDevice;
    int inputChannel = 0;
    double sampleRate = 0.0;
    int bufferSize = 0;
};

class AudioDeviceService final {
public:
    [[nodiscard]] static juce::var discover();
    [[nodiscard]] static std::optional<juce::var> probeDeviceChannels(
        const juce::String& driver, const juce::String& inputDevice,
        const juce::String& outputDevice, juce::String& error);
    [[nodiscard]] static juce::String initialise(juce::AudioDeviceManager& manager,
                                                 const AudioConfiguration& configuration);
    [[nodiscard]] static juce::var currentStatus(juce::AudioDeviceManager& manager,
                                                 const SafetyAudioCallback& callback,
                                                 const MidiMonitor* midi = nullptr,
                                                 const juce::String& message = {},
                                                 const TimelineEngine* timeline = nullptr);
    [[nodiscard]] static juce::var currentMeters(const SafetyAudioCallback& callback);

private:
    [[nodiscard]] static juce::String accessModeForDriver(const juce::String& driver);
    [[nodiscard]] static bool driverRequiresSameDevice(const juce::String& driver);
    [[nodiscard]] static juce::String defaultDriver();
};

class DeviceFaultWatcher final : public juce::ChangeListener {
public:
    DeviceFaultWatcher(juce::AudioDeviceManager& manager, SafetyAudioCallback& callback,
                       TimelineEngine& timeline);

    void changeListenerCallback(juce::ChangeBroadcaster*) override;

private:
    juce::AudioDeviceManager& deviceManager;
    SafetyAudioCallback& audioCallback;
    TimelineEngine& timelineEngine;
};

}  // namespace riffra
