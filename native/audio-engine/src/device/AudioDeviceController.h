#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <functional>

#include "AudioConfiguration.h"
#include "AudioDeviceCallback.h"

namespace riffra {

class AudioRenderPipeline;

struct AudioDeviceSwitchResult {
    juce::String setupError;
    juce::String restoreError;
    bool inputChannelUnavailable = false;
    bool restoredPreviousDevice = false;
};

/// Owns the live JUCE audio-device lifecycle and transition state.
class AudioDeviceController final : public juce::ChangeListener {
public:
    using DeviceLossHandler = std::function<void()>;

    explicit AudioDeviceController(
        AudioRenderPipeline& pipeline,
        AudioDeviceCallback::DeviceStoppedHandler deviceStoppedHandler = {});
    ~AudioDeviceController() override;

    AudioDeviceController(const AudioDeviceController&) = delete;
    AudioDeviceController& operator=(const AudioDeviceController&) = delete;

    // Control thread only.
    [[nodiscard]] juce::String initialise(const AudioConfiguration& configuration);
    void attach();
    void close();
    [[nodiscard]] juce::String recover();
    [[nodiscard]] AudioDeviceSwitchResult switchDevice(const AudioConfiguration& configuration);
    void setDeviceLossHandler(DeviceLossHandler handler);
    [[nodiscard]] juce::AudioDeviceManager& manager() noexcept { return deviceManager; }
    [[nodiscard]] const juce::AudioDeviceManager& manager() const noexcept { return deviceManager; }
    [[nodiscard]] AudioDeviceCallback& callback() noexcept { return deviceCallback; }
    [[nodiscard]] juce::AudioIODevice* currentDevice() const noexcept;
    [[nodiscard]] juce::String currentDeviceType() const;
    [[nodiscard]] bool isTransitionActive() const noexcept;

    [[nodiscard]] static bool requiresFaultForState(bool devicePresent,
                                                    bool transitionActive) noexcept;

    // JUCE device change notification. The callback runs on the control-side
    // device lifecycle path and never performs audio rendering.
    void changeListenerCallback(juce::ChangeBroadcaster*) override;

private:
    void detach() noexcept;
    [[nodiscard]] juce::String restorePreviousDevice(
        const juce::String& previousDriver,
        const juce::AudioDeviceManager::AudioDeviceSetup& previousSetup, int previousInputChannel,
        bool& restoredPreviousDevice);

    AudioRenderPipeline& renderPipeline;
    juce::AudioDeviceManager deviceManager;
    AudioDeviceCallback deviceCallback;
    std::atomic<bool> transitionActive{false};
    bool callbackAttached = false;
    bool listenerAttached = false;
    DeviceLossHandler deviceLossHandler;
};

}  // namespace riffra
