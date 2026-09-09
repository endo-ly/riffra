#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <memory>
#include <optional>

namespace riffra {

class MidiMonitor;
class AudioRenderPipeline;
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
    // Control thread only. This service discovers devices and prepares setup
    // data; it does not own the live device lifecycle.
    [[nodiscard]] static juce::var discover();
    [[nodiscard]] static std::optional<juce::var> probeDeviceChannels(
        const juce::String& driver, const juce::String& inputDevice,
        const juce::String& outputDevice, juce::String& error);
    [[nodiscard]] static juce::String initialise(juce::AudioDeviceManager& manager,
                                                 const AudioConfiguration& configuration);
    [[nodiscard]] static juce::var currentStatus(juce::AudioDeviceManager& manager,
                                                 const AudioRenderPipeline& pipeline,
                                                 const MidiMonitor* midi = nullptr,
                                                 const juce::String& message = {},
                                                 TimelineEngine* timeline = nullptr);
    [[nodiscard]] static juce::var currentMeters(const AudioRenderPipeline& pipeline);

private:
    [[nodiscard]] static juce::String accessModeForDriver(const juce::String& driver);
    [[nodiscard]] static bool driverRequiresSameDevice(const juce::String& driver);
    [[nodiscard]] static juce::String defaultDriver();
};

class DeviceFaultWatcher final : public juce::ChangeListener {
public:
    DeviceFaultWatcher(juce::AudioDeviceManager& manager, AudioRenderPipeline& pipeline,
                       TimelineEngine& timeline);

    void changeListenerCallback(juce::ChangeBroadcaster*) override;

private:
    juce::AudioDeviceManager& deviceManager;
    AudioRenderPipeline& renderPipeline;
    TimelineEngine& timelineEngine;
};

}  // namespace riffra
