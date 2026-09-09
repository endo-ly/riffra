#include "../AudioCommandDispatcher.h"
#include "device/AudioDeviceController.h"
#include "midi/MidiInputService.h"
#include "protocol/AudioProtocol.h"

namespace riffra {

CommandResult AudioCommandDispatcher::dispatchDevice(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "recoverAudioDevice") {
        const auto recoveryError = context.deviceController.recover();
        if (recoveryError.isNotEmpty()) {
            writeJson(makeError("deviceLost", recoveryError, "audioDevice.recover"));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "setAudioDriver") {
        const auto driver = command.getProperty("driver", {}).toString();
        if (driver.isEmpty()) {
            writeJson(makeError("invalidAudioConfiguration", "An audio driver name is required.",
                                "audioDevice.validate"));
            return {};
        }
        AudioConfiguration requested;
        requested.driver = driver;
        requested.inputDevice = command.getProperty("inputDevice", {}).toString();
        requested.outputDevice = command.getProperty("outputDevice", {}).toString();
        requested.inputChannel = static_cast<int>(command.getProperty("inputChannel", 0));
        if (requested.inputChannel < 0) {
            writeJson(makeError("invalidAudioConfiguration",
                                "Input channel must be zero or greater.", "audioDevice.validate"));
            return {};
        }
        requested.sampleRate = static_cast<double>(command.getProperty("sampleRate", 0.0));
        requested.bufferSize = static_cast<int>(command.getProperty("bufferSize", 0));
        const auto switchResult = context.deviceController.switchDevice(requested);
        if (switchResult.setupError.isNotEmpty()) {
            auto* details = new juce::DynamicObject();
            details->setProperty("driver", requested.driver);
            details->setProperty("inputDevice", requested.inputDevice);
            details->setProperty("outputDevice", requested.outputDevice);
            details->setProperty("restoredPreviousDevice", switchResult.restoredPreviousDevice);
            writeJson(makeError(
                switchResult.restoredPreviousDevice ? "deviceRejected" : "deviceLost",
                switchResult.setupError + (switchResult.restoreError.isEmpty()
                                               ? ". The previous device was restored."
                                               : ". The previous device could not be restored: " +
                                                     switchResult.restoreError),
                "audioDevice.activate", juce::var(details)));
            return {};
        }
        if (switchResult.inputChannelUnavailable) {
            auto* details = new juce::DynamicObject();
            details->setProperty("driver", requested.driver);
            details->setProperty("inputDevice", requested.inputDevice);
            details->setProperty("outputDevice", requested.outputDevice);
            details->setProperty("restoredPreviousDevice", switchResult.restoredPreviousDevice);
            const auto message =
                juce::String("The selected physical input channel is unavailable.") +
                (switchResult.restoreError.isEmpty()
                     ? " The previous device was restored."
                     : " The previous device could not be restored: " + switchResult.restoreError);
            writeJson(
                makeError(switchResult.restoredPreviousDevice ? "deviceRejected" : "deviceLost",
                          message, "audioDevice.activate", juce::var(details)));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    return {};
}

}  // namespace riffra
