#include <JuceHeader.h>

#include "SafetyAudioCallback.h"
#include "RecordingSelfTest.h"
#include "PluginRack.h"

#include <iostream>
#include <memory>
#include <cmath>
#include <limits>

namespace {

using riffra::SafetyAudioCallback;
using riffra::PluginRack;

juce::var makeError(const juce::String& scope, const juce::String& message) {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "error");
    object->setProperty("scope", scope);
    object->setProperty("message", message);
    object->setProperty("dataSafe", true);
    return juce::var(object);
}

void writeJson(const juce::var& value) {
    std::cout << juce::JSON::toString(value, true) << std::endl;
}

juce::var probeAudioDevices() {
    juce::AudioDeviceManager manager;
    juce::OwnedArray<juce::AudioIODeviceType> types;
    manager.createAudioDeviceTypes(types);

    juce::Array<juce::var> driverTypes;
    for (auto* type : types) {
        type->scanForDevices();
        auto* driver = new juce::DynamicObject();
        driver->setProperty("name", type->getTypeName());

        juce::Array<juce::var> inputs;
        for (const auto& name : type->getDeviceNames(true))
            inputs.add(name);
        driver->setProperty("inputs", inputs);

        juce::Array<juce::var> outputs;
        for (const auto& name : type->getDeviceNames(false))
            outputs.add(name);
        driver->setProperty("outputs", outputs);
        driverTypes.add(juce::var(driver));
    }

    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(device.name);
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(device.name);

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "audioDeviceProbe");
    result->setProperty("drivers", driverTypes);
    result->setProperty("emergencyMuted", true);
    result->setProperty("startupGainDb", -18.0);
    result->setProperty("limiterCeiling", 0.98);
    result->setProperty("midiInputs", midiInputs);
    result->setProperty("midiOutputs", midiOutputs);
    return juce::var(result);
}

juce::var currentStatus(juce::AudioDeviceManager& manager, const SafetyAudioCallback& callback, const PluginRack* rack = nullptr) {
    auto* status = new juce::DynamicObject();
    status->setProperty("type", "audioStatus");
    status->setProperty("state", callback.isEmergencyMuted() ? "muted" : "ready");
    status->setProperty("emergencyMuted", callback.isEmergencyMuted());
    status->setProperty("masterGainDb", callback.getMasterGainDb());
    status->setProperty("inputPeak", callback.getInputPeak());
    status->setProperty("outputPeak", callback.getOutputPeak());
    status->setProperty("invalidSamples", static_cast<juce::int64>(callback.getInvalidSampleCount()));
    status->setProperty("previewing", callback.isPreviewing());
    status->setProperty("recording", callback.recordingStatus());

    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(device.name);
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(device.name);
    status->setProperty("midiInputs", midiInputs);
    status->setProperty("midiOutputs", midiOutputs);

    if (auto* device = manager.getCurrentAudioDevice()) {
        status->setProperty("driver", device->getTypeName());
        status->setProperty("inputDevice", device->getName());
        status->setProperty("outputDevice", device->getName());
        status->setProperty("sampleRate", device->getCurrentSampleRate());
        status->setProperty("bufferSize", device->getCurrentBufferSizeSamples());
        const auto latencySamples = device->getInputLatencyInSamples() + device->getOutputLatencyInSamples();
        const auto latencyMs = device->getCurrentSampleRate() > 0.0
            ? 1000.0 * static_cast<double>(latencySamples) / device->getCurrentSampleRate()
            : 0.0;
        status->setProperty("roundTripMs", latencyMs);
    }
    if (rack != nullptr)
        status->setProperty("plugin", rack->status());
    return juce::var(status);
}

juce::String initialiseDefaultAudio(juce::AudioDeviceManager& manager) {
    auto error = manager.initialiseWithDefaultDevices(2, 2);
    if (error.isNotEmpty())
        error = manager.initialiseWithDefaultDevices(0, 2);
    return error;
}

int serve() {
    juce::AudioDeviceManager manager;
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    SafetyAudioCallback callback;
    PluginRack rack;
    callback.setPluginRack(&rack);
    callback.setEmergencyMuted(true);
    callback.setMasterGainDb(-18.0f);

    auto error = initialiseDefaultAudio(manager);
    if (error.isNotEmpty()) {
        writeJson(makeError("audioDevice", error));
        return 2;
    }

    manager.addAudioCallback(&callback);
    writeJson(currentStatus(manager, callback, &rack));

    std::string line;
    while (std::getline(std::cin, line)) {
        const auto command = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
        if (!command.isObject()) {
            writeJson(makeError("protocol", "Expected one JSON object per line."));
            continue;
        }

        const auto type = command.getProperty("type", {}).toString();
        if (type == "shutdown")
            break;
        if (type == "setEmergencyMute") {
            callback.setEmergencyMuted(static_cast<bool>(command.getProperty("muted", true)));
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "setMasterGainDb") {
            callback.setMasterGainDb(static_cast<float>(command.getProperty("gainDb", -18.0)));
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "loadPlugin") {
            const auto path = command.getProperty("path", {}).toString();
            auto* device = manager.getCurrentAudioDevice();
            const auto sampleRate = callback.getSampleRate();
            const auto blockSize = device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
            juce::String pluginError;
            if (path.isEmpty() || sampleRate <= 0.0 || blockSize <= 0
                || !rack.load(path, sampleRate, blockSize, pluginError)) {
                writeJson(makeError(
                    "plugin",
                    path.isEmpty() ? "VST3 path is required." : pluginError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "clearPlugin") {
            rack.clear();
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "probeMidiDevices") {
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "previewSample") {
            const auto path = command.getProperty("path", {}).toString();
            std::unique_ptr<juce::AudioFormatReader> reader(
                path.isEmpty() ? nullptr : formatManager.createReaderFor(juce::File(path)));
            juce::String previewError;
            const auto sampleRate = callback.getSampleRate();
            if (reader == nullptr) {
                previewError = "Preview source could not be opened as an audio file.";
            } else if (sampleRate <= 0.0 || std::abs(reader->sampleRate - sampleRate) > 0.5) {
                previewError = "Preview source sample rate does not match the active audio device.";
            } else {
                const auto length = juce::jmin<juce::int64>(
                    reader->lengthInSamples,
                    static_cast<juce::int64>(std::numeric_limits<int>::max()));
                juce::AudioBuffer<float> buffer(reader->numChannels, static_cast<int>(length));
                if (length <= 0 || !reader->read(
                    &buffer,
                    0,
                    static_cast<int>(length),
                    0,
                    true,
                    true)) {
                    previewError = "Preview source contains no readable audio samples.";
                } else {
                    const auto startMs = static_cast<double>(command.getProperty("startMs", 0.0));
                    const auto endMs = static_cast<double>(command.getProperty("endMs", -1.0));
                    const auto start = juce::jlimit(
                        0,
                        static_cast<int>(length),
                        static_cast<int>(std::llround(startMs * reader->sampleRate / 1000.0)));
                    const auto end = endMs <= 0.0
                        ? static_cast<int>(length)
                        : juce::jlimit(
                            start + 1,
                            static_cast<int>(length),
                            static_cast<int>(std::llround(endMs * reader->sampleRate / 1000.0)));
                    if (!callback.startPreview(
                        buffer,
                        start,
                        end,
                        static_cast<float>(static_cast<double>(command.getProperty("gain", 1.0))),
                        previewError))
                        previewError = previewError.isEmpty() ? "Preview range is invalid." : previewError;
                }
            }
            if (previewError.isNotEmpty()) {
                writeJson(makeError("preview", previewError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "stopPreview") {
            callback.stopPreview();
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "setPluginBypassed") {
            rack.setBypassed(static_cast<bool>(command.getProperty("bypassed", true)));
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "recoverAudioDevice") {
            manager.removeAudioCallback(&callback);
            manager.closeAudioDevice();
            callback.setEmergencyMuted(true);
            const auto recoveryError = initialiseDefaultAudio(manager);
            if (recoveryError.isNotEmpty()) {
                writeJson(makeError("audioDevice", recoveryError));
                continue;
            }
            manager.addAudioCallback(&callback);
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "setAudioDriver") {
            const auto driver = command.getProperty("driver", {}).toString();
            if (driver.isEmpty()) {
                writeJson(makeError("audioDevice", "An audio driver name is required."));
                continue;
            }
            manager.removeAudioCallback(&callback);
            manager.closeAudioDevice();
            callback.setEmergencyMuted(true);
            manager.setCurrentAudioDeviceType(driver, true);
            juce::AudioDeviceManager::AudioDeviceSetup setup;
            setup.inputDeviceName = command.getProperty("inputDevice", {}).toString();
            setup.outputDeviceName = command.getProperty("outputDevice", {}).toString();
            setup.sampleRate = static_cast<double>(command.getProperty("sampleRate", 0.0));
            setup.bufferSize = static_cast<int>(command.getProperty("bufferSize", 0));
            setup.useDefaultInputChannels = true;
            setup.useDefaultOutputChannels = true;
            auto setupError = setup.inputDeviceName.isEmpty() && setup.outputDeviceName.isEmpty()
                ? initialiseDefaultAudio(manager)
                : manager.setAudioDeviceSetup(setup, true);
            if (setupError.isNotEmpty()) {
                const auto fallbackError = initialiseDefaultAudio(manager);
                if (fallbackError.isEmpty())
                    manager.addAudioCallback(&callback);
                writeJson(makeError("audioDevice", setupError + ". The previous device was not changed in the session."));
                continue;
            }
            manager.addAudioCallback(&callback);
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "startRecording") {
            const auto directory = command.getProperty("directory", {}).toString();
            juce::String recordingError;
            if (directory.isEmpty() || !callback.startRecording(juce::File(directory), recordingError)) {
                writeJson(makeError("recording", directory.isEmpty() ? "Recording directory is required." : recordingError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "stopRecording") {
            juce::String recordingError;
            if (!callback.stopRecording(recordingError)) {
                writeJson(makeError("recording", recordingError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        if (type == "status") {
            writeJson(currentStatus(manager, callback, &rack));
            continue;
        }
        writeJson(makeError("protocol", "Unsupported command: " + type));
    }

    callback.setEmergencyMuted(true);
    manager.removeAudioCallback(&callback);
    manager.closeAudioDevice();
    return 0;
}

} // namespace
int main(int argc, char* argv[]) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (argc < 2) {
        writeJson(makeError("arguments", "Use --probe or --serve."));
        return 1;
    }
    const juce::String command(argv[1]);
    if (command == "--probe") {
        writeJson(probeAudioDevices());
        return 0;
    }
    if (command == "--recording-self-test") {
        if (argc < 3) {
            writeJson(makeError("arguments", "Use --recording-self-test <directory>."));
            return 1;
        }
        writeJson(riffra::runRecordingSelfTest(juce::File(argv[2])));
        return 0;
    }
    if (command == "--serve")
        return serve();
    writeJson(makeError("arguments", "Unknown command: " + command));
    return 1;
}
