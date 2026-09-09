#include "AudioDeviceController.h"

#include <utility>

#include "AudioDeviceService.h"
#include "audio/AudioRenderPipeline.h"

namespace riffra {

AudioDeviceController::AudioDeviceController(
    AudioRenderPipeline& pipeline, AudioDeviceCallback::DeviceStoppedHandler deviceStoppedHandler)
    : renderPipeline(pipeline), deviceCallback(pipeline, std::move(deviceStoppedHandler)) {}

AudioDeviceController::~AudioDeviceController() { close(); }

juce::String AudioDeviceController::initialise(const AudioConfiguration& configuration) {
    return AudioDeviceService::initialise(deviceManager, configuration);
}

void AudioDeviceController::attach() {
    if (!callbackAttached) {
        deviceManager.addAudioCallback(&deviceCallback);
        callbackAttached = true;
    }
    if (!listenerAttached) {
        deviceManager.addChangeListener(this);
        listenerAttached = true;
    }
}

void AudioDeviceController::detach() noexcept {
    if (callbackAttached) {
        deviceManager.removeAudioCallback(&deviceCallback);
        callbackAttached = false;
    }
    if (listenerAttached) {
        deviceManager.removeChangeListener(this);
        listenerAttached = false;
    }
}

void AudioDeviceController::close() {
    detach();
    deviceManager.closeAudioDevice();
}

juce::AudioIODevice* AudioDeviceController::currentDevice() const noexcept {
    return deviceManager.getCurrentAudioDevice();
}

juce::String AudioDeviceController::currentDeviceType() const {
    return deviceManager.getCurrentAudioDeviceType();
}

bool AudioDeviceController::isTransitionActive() const noexcept {
    return transitionActive.load(std::memory_order_acquire);
}

bool AudioDeviceController::requiresFaultForState(const bool devicePresent,
                                                  const bool transitionActiveIn) noexcept {
    return !devicePresent && !transitionActiveIn;
}

juce::String AudioDeviceController::recover() {
    juce::AudioDeviceManager::AudioDeviceSetup recoverySetup;
    deviceManager.getAudioDeviceSetup(recoverySetup);
    transitionActive.store(true, std::memory_order_release);
    const auto wasAttached = callbackAttached;
    if (wasAttached) deviceManager.removeAudioCallback(&deviceCallback);
    callbackAttached = false;
    deviceManager.closeAudioDevice();
    renderPipeline.setEngineTransitionMute(true);
    const auto error = deviceManager.setAudioDeviceSetup(recoverySetup, true);
    if (error.isNotEmpty()) {
        transitionActive.store(false, std::memory_order_release);
        renderPipeline.setDeviceFaulted(true);
        renderPipeline.setEngineTransitionMute(false);
        return error;
    }
    attach();
    renderPipeline.setDeviceFaulted(false);
    transitionActive.store(false, std::memory_order_release);
    return {};
}

juce::String AudioDeviceController::restorePreviousDevice(
    const juce::String& previousDriver,
    const juce::AudioDeviceManager::AudioDeviceSetup& previousSetup, const int previousInputChannel,
    bool& restoredPreviousDevice) {
    deviceManager.closeAudioDevice();
    AudioConfiguration previous;
    previous.driver = previousDriver;
    previous.inputDevice = previousSetup.inputDeviceName;
    previous.outputDevice = previousSetup.outputDeviceName;
    previous.inputChannel = previousInputChannel;
    previous.sampleRate = previousSetup.sampleRate;
    previous.bufferSize = previousSetup.bufferSize;
    const auto restoreError = AudioDeviceService::initialise(deviceManager, previous);
    if (restoreError.isEmpty()) {
        renderPipeline.setInputChannel(previousInputChannel);
        attach();
        renderPipeline.setDeviceFaulted(false);
        transitionActive.store(false, std::memory_order_release);
        restoredPreviousDevice = true;
    } else {
        transitionActive.store(false, std::memory_order_release);
        renderPipeline.setDeviceFaulted(true);
    }
    return restoreError;
}

AudioDeviceSwitchResult AudioDeviceController::switchDevice(const AudioConfiguration& requested) {
    AudioDeviceSwitchResult result;
    const auto previousDriver = currentDeviceType();
    const auto previousInputChannel = renderPipeline.getInputChannel();
    juce::AudioDeviceManager::AudioDeviceSetup previousSetup;
    deviceManager.getAudioDeviceSetup(previousSetup);
    transitionActive.store(true, std::memory_order_release);
    detach();
    deviceManager.closeAudioDevice();
    renderPipeline.setEngineTransitionMute(true);

    result.setupError = AudioDeviceService::initialise(deviceManager, requested);
    if (result.setupError.isNotEmpty()) {
        result.restoreError = restorePreviousDevice(
            previousDriver, previousSetup, previousInputChannel, result.restoredPreviousDevice);
        renderPipeline.setEngineTransitionMute(false);
        return result;
    }

    auto* activeDevice = deviceManager.getCurrentAudioDevice();
    const auto physicalInputs =
        activeDevice != nullptr ? activeDevice->getInputChannelNames().size() : 0;
    if (requested.inputChannel >= physicalInputs) {
        result.inputChannelUnavailable = true;
        result.restoreError = restorePreviousDevice(
            previousDriver, previousSetup, previousInputChannel, result.restoredPreviousDevice);
        renderPipeline.setEngineTransitionMute(false);
        return result;
    }

    renderPipeline.setInputChannel(requested.inputChannel);
    attach();
    renderPipeline.setDeviceFaulted(false);
    transitionActive.store(false, std::memory_order_release);
    return result;
}

void AudioDeviceController::setDeviceLossHandler(DeviceLossHandler handler) {
    deviceLossHandler = std::move(handler);
}

void AudioDeviceController::changeListenerCallback(juce::ChangeBroadcaster*) {
    if (!requiresFaultForState(currentDevice() != nullptr, isTransitionActive())) return;
    if (renderPipeline.isDeviceFaulted()) return;
    renderPipeline.setDeviceFaulted(true);
    juce::String ignored;
    (void)renderPipeline.recording().stop(ignored);
    if (deviceLossHandler != nullptr) deviceLossHandler();
}

}  // namespace riffra
