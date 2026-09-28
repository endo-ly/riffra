#include "../AudioCommandDispatcher.h"
#include "device/AudioDeviceController.h"

namespace riffra {
namespace {

void failDeviceSwitch(CommandResponder& responder, const AudioConfiguration& requested,
                      const AudioDeviceSwitchResult& result, const juce::String& cause) {
    responder.fail(
        result.restoredPreviousDevice ? "deviceRejected" : "deviceLost",
        cause + (result.restoreError.isEmpty()
                     ? " The previous device was restored."
                     : " The previous device could not be restored: " + result.restoreError),
        "audioDevice.activate",
        encodeDeviceSwitchDetails({requested.driver, requested.inputDevice, requested.outputDevice,
                                   result.restoredPreviousDevice}));
}

}  // namespace

void AudioCommandDispatcher::handle(const RecoverAudioDeviceCommand&, CommandResponder responder) {
    const auto recoveryError = context.deviceController.recover();
    if (recoveryError.isNotEmpty()) {
        responder.fail("deviceLost", recoveryError, "audioDevice.recover");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetAudioDriverCommand& command,
                                    CommandResponder responder) {
    if (command.driver.isEmpty()) {
        responder.fail("invalidAudioConfiguration", "An audio driver name is required.",
                       "audioDevice.validate");
        return;
    }
    AudioConfiguration requested;
    requested.driver = command.driver;
    requested.inputDevice = command.inputDevice.value_or(juce::String());
    requested.outputDevice = command.outputDevice.value_or(juce::String());
    requested.inputChannel = static_cast<int>(command.inputChannel);
    requested.sampleRate = static_cast<double>(command.sampleRate.value_or(0));
    requested.bufferSize = static_cast<int>(command.bufferSize.value_or(0));
    const auto switchResult = context.deviceController.switchDevice(requested);
    if (switchResult.setupError.isNotEmpty()) {
        failDeviceSwitch(responder, requested, switchResult, switchResult.setupError + ".");
        return;
    }
    if (switchResult.inputChannelUnavailable) {
        failDeviceSwitch(responder, requested, switchResult,
                         "The selected physical input channel is unavailable.");
        return;
    }
    responder.respond(currentStatus());
}

}  // namespace riffra
