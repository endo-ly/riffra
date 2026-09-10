#include "AudioDeviceCallback.h"

#include <utility>

#include "audio/AudioRenderPipeline.h"

namespace riffra {

AudioDeviceCallback::AudioDeviceCallback(AudioRenderPipeline& pipeline,
                                         DeviceStoppedHandler deviceStoppedHandlerIn)
    : renderPipeline(pipeline), deviceStoppedHandler(std::move(deviceStoppedHandlerIn)) {}

void AudioDeviceCallback::audioDeviceIOCallbackWithContext(
    const float* const* inputChannelData, const int numInputChannels,
    float* const* outputChannelData, const int numOutputChannels, const int numSamples,
    const juce::AudioIODeviceCallbackContext& context) {
    renderPipeline.processBlock(inputChannelData, numInputChannels, outputChannelData,
                                numOutputChannels, numSamples, context);
}

void AudioDeviceCallback::audioDeviceAboutToStart(juce::AudioIODevice* const device) {
    renderPipeline.prepare(device);
}

void AudioDeviceCallback::audioDeviceStopped() {
    renderPipeline.deviceStopped();
    if (deviceStoppedHandler != nullptr) deviceStoppedHandler();
}

void AudioDeviceCallback::audioDeviceError(const juce::String& errorMessage) {
    const juce::ScopedLock guard(errorLock);
    lastDeviceError = errorMessage;
    renderPipeline.setDeviceFaulted(true);
}

juce::String AudioDeviceCallback::takeLastDeviceError() {
    const juce::ScopedLock guard(errorLock);
    return std::exchange(lastDeviceError, {});
}

}  // namespace riffra
