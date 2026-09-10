#pragma once

#include <JuceHeader.h>

#include <functional>

namespace riffra {

class AudioRenderPipeline;

/// Adapts JUCE's device callback interface to the realtime render pipeline.
class AudioDeviceCallback final : public juce::AudioIODeviceCallback {
public:
    using DeviceStoppedHandler = std::function<void()>;

    // Construct before the device callbacks are attached.
    explicit AudioDeviceCallback(AudioRenderPipeline& pipeline,
                                 DeviceStoppedHandler deviceStoppedHandler = {});
    ~AudioDeviceCallback() override = default;

    AudioDeviceCallback(const AudioDeviceCallback&) = delete;
    AudioDeviceCallback& operator=(const AudioDeviceCallback&) = delete;

    // Audio thread only. The adapter performs no processing of its own.
    void audioDeviceIOCallbackWithContext(
        const float* const* inputChannelData, int numInputChannels, float* const* outputChannelData,
        int numOutputChannels, int numSamples,
        const juce::AudioIODeviceCallbackContext& context) override;

    // Device lifecycle/control side only; separate from the audio callback.
    void audioDeviceAboutToStart(juce::AudioIODevice* device) override;
    void audioDeviceStopped() override;
    void audioDeviceError(const juce::String& errorMessage) override;

    [[nodiscard]] juce::String takeLastDeviceError();

private:
    AudioRenderPipeline& renderPipeline;
    DeviceStoppedHandler deviceStoppedHandler;
    juce::CriticalSection errorLock;
    juce::String lastDeviceError;
};

}  // namespace riffra
