#include <JuceHeader.h>

#include "SafetyAudioCallback.h"
#include "RecordingSelfTest.h"
#include "PluginRack.h"

#include <iostream>
#include <map>
#include <memory>
#include <cmath>
#include <limits>
#include <vector>

namespace {

using riffra::SafetyAudioCallback;
using riffra::PluginRack;

class MidiMonitor final : public juce::MidiInputCallback {
public:
    struct Pad {
        std::shared_ptr<juce::AudioBuffer<float>> buffer;
        int start = 0;
        int end = 0;
        float gain = 1.0f;
        bool loop = false;
    };

    struct RecordedEvent {
        double timeMs = 0.0;
        int status = 0;
        int channel = 0;
        int note = 0;
        int velocity = 0;
    };

    void setAudioCallback(SafetyAudioCallback* const callback) noexcept { audioCallback = callback; }

    void replacePads(std::map<int, Pad>&& next) {
        const juce::ScopedLock lock(padLock);
        pads = std::move(next);
    }

    void beginRecording(const juce::File& file) {
        const juce::ScopedLock lock(recordingLock);
        recordedEvents.clear();
        recordingFile = file;
        recordingStartMs = juce::Time::getMillisecondCounterHiRes();
        recordingMidi.store(true, std::memory_order_release);
    }

    bool finishRecording(juce::String& error) {
        std::vector<RecordedEvent> events;
        juce::File file;
        {
            const juce::ScopedLock lock(recordingLock);
            recordingMidi.store(false, std::memory_order_release);
            events = recordedEvents;
            file = recordingFile;
            recordingFile = {};
        }
        if (file == juce::File())
            return true;
        if (!file.getParentDirectory().createDirectory()) {
            error = "MIDI recording destination could not be created.";
            return false;
        }
        juce::Array<juce::var> encoded;
        for (const auto& event : events) {
            auto* object = new juce::DynamicObject();
            object->setProperty("timeMs", event.timeMs);
            object->setProperty("status", event.status);
            object->setProperty("channel", event.channel);
            object->setProperty("note", event.note);
            object->setProperty("velocity", event.velocity);
            encoded.add(juce::var(object));
        }
        auto* root = new juce::DynamicObject();
        root->setProperty("version", 1);
        root->setProperty("events", encoded);
        if (!file.replaceWithText(juce::JSON::toString(juce::var(root), true))) {
            error = "MIDI recording JSON could not be finalized.";
            return false;
        }
        return true;
    }

    void handleIncomingMidiMessage(juce::MidiInput*, const juce::MidiMessage& message) override {
        messageCount.fetch_add(1, std::memory_order_relaxed);
        if (message.isNoteOn() || message.isNoteOff()) {
            const juce::ScopedLock lock(recordingLock);
            if (recordingMidi.load(std::memory_order_acquire)
                && recordedEvents.size() < 200'000) {
                recordedEvents.push_back(RecordedEvent {
                    juce::Time::getMillisecondCounterHiRes() - recordingStartMs,
                    message.getRawDataSize() > 0 ? message.getRawData()[0] & 0xf0 : 0,
                    message.getChannel(),
                    message.getNoteNumber(),
                    message.getVelocity(),
                });
            }
        }
        if (!message.isNoteOn() && !message.isNoteOff())
            return;

        lastNote.store(message.getNoteNumber(), std::memory_order_release);
        if (message.isNoteOff()) {
            if (audioCallback != nullptr) {
                audioCallback->stopPreviewForKey(message.getNoteNumber());
                audioCallback->stopSynthNote(message.getNoteNumber());
            }
            return;
        }

        std::shared_ptr<juce::AudioBuffer<float>> buffer;
        int start = 0;
        int end = 0;
        float gain = 1.0f;
        bool loop = false;
        {
            const juce::ScopedLock lock(padLock);
            const auto found = pads.find(message.getNoteNumber());
            if (found != pads.end()) {
                buffer = found->second.buffer;
                start = found->second.start;
                end = found->second.end;
                gain = found->second.gain;
                loop = found->second.loop;
            }
        }
        if (audioCallback == nullptr)
            return;

        if (buffer == nullptr) {
            audioCallback->startSynthNote(message.getNoteNumber(), message.getFloatVelocity());
            return;
        }

        juce::String error;
        if (audioCallback->startPreview(
                *buffer,
                start,
                end,
                juce::jlimit(0.05f, 1.0f, message.getFloatVelocity()) * gain,
                loop,
                error,
                message.getNoteNumber()))
            padTriggers.fetch_add(1, std::memory_order_relaxed);
    }

    void setActive(const bool value) noexcept { active.store(value, std::memory_order_release); }
    [[nodiscard]] bool isActive() const noexcept { return active.load(std::memory_order_acquire); }
    [[nodiscard]] std::uint64_t getMessageCount() const noexcept { return messageCount.load(std::memory_order_acquire); }
    [[nodiscard]] int getLastNote() const noexcept { return lastNote.load(std::memory_order_acquire); }
    [[nodiscard]] int getPadMappingCount() const noexcept {
        const juce::ScopedLock lock(padLock);
        return static_cast<int>(pads.size());
    }
    [[nodiscard]] std::uint64_t getPadTriggerCount() const noexcept { return padTriggers.load(std::memory_order_acquire); }
    [[nodiscard]] bool isRecording() const noexcept { return recordingMidi.load(std::memory_order_acquire); }
    [[nodiscard]] std::size_t getRecordedEventCount() const noexcept {
        const juce::ScopedLock lock(recordingLock);
        return recordedEvents.size();
    }

private:
    std::atomic<bool> active { false };
    std::atomic<std::uint64_t> messageCount { 0 };
    std::atomic<int> lastNote { -1 };
    std::atomic<std::uint64_t> padTriggers { 0 };
    std::atomic<bool> recordingMidi { false };
    SafetyAudioCallback* audioCallback = nullptr;
    mutable juce::CriticalSection padLock;
    std::map<int, Pad> pads;
    mutable juce::CriticalSection recordingLock;
    juce::File recordingFile;
    double recordingStartMs = 0.0;
    std::vector<RecordedEvent> recordedEvents;
};

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

juce::var currentStatus(juce::AudioDeviceManager& manager, const SafetyAudioCallback& callback, const PluginRack* rack = nullptr, const MidiMonitor* midi = nullptr) {
    auto* status = new juce::DynamicObject();
    status->setProperty("type", "audioStatus");
    status->setProperty("state", callback.isEmergencyMuted() ? "muted" : "ready");
    status->setProperty("emergencyMuted", callback.isEmergencyMuted());
    status->setProperty("masterGainDb", callback.getMasterGainDb());
    status->setProperty("inputPeak", callback.getInputPeak());
    status->setProperty("outputPeak", callback.getOutputPeak());
    status->setProperty("invalidSamples", static_cast<juce::int64>(callback.getInvalidSampleCount()));
    status->setProperty("previewing", callback.isPreviewing());
    if (midi != nullptr) {
        status->setProperty("midiInputActive", midi->isActive());
        status->setProperty("midiMessages", static_cast<juce::int64>(midi->getMessageCount()));
        status->setProperty("lastMidiNote", midi->getLastNote());
        status->setProperty("midiPadMappings", midi->getPadMappingCount());
        status->setProperty("midiPadTriggers", static_cast<juce::int64>(midi->getPadTriggerCount()));
        status->setProperty("midiRecording", midi->isRecording());
        status->setProperty("midiRecordedEvents", static_cast<juce::int64>(midi->getRecordedEventCount()));
    }
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
    MidiMonitor midiMonitor;
    std::unique_ptr<juce::MidiInput> midiInput;
    callback.setPluginRack(&rack);
    midiMonitor.setAudioCallback(&callback);
    callback.setEmergencyMuted(true);
    callback.setMasterGainDb(-18.0f);

    auto error = initialiseDefaultAudio(manager);
    if (error.isNotEmpty()) {
        writeJson(makeError("audioDevice", error));
        return 2;
    }

    manager.addAudioCallback(&callback);
    writeJson(currentStatus(manager, callback, &rack, &midiMonitor));

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
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "setMasterGainDb") {
            callback.setMasterGainDb(static_cast<float>(command.getProperty("gainDb", -18.0)));
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
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
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "clearPlugin") {
            rack.clear();
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "probeMidiDevices") {
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "configureSamplePads") {
            const auto padsValue = command.getProperty("pads", {});
            const auto sampleRate = callback.getSampleRate();
            juce::String mappingError;
            std::map<int, MidiMonitor::Pad> nextPads;
            if (!padsValue.isArray()) {
                mappingError = "Sample pad mappings must be an array.";
            } else if (sampleRate <= 0.0) {
                mappingError = "Sample pad mappings require an active audio device.";
            } else {
                for (const auto& item : *padsValue.getArray()) {
                    const auto path = item.getProperty("assetPath", {}).toString();
                    const auto midiKey = static_cast<int>(item.getProperty("midiKey", -1));
                    if (path.isEmpty() || midiKey < 0 || midiKey > 127) {
                        mappingError = "Each sample pad requires a source path and MIDI key 0-127.";
                        break;
                    }
                    std::unique_ptr<juce::AudioFormatReader> reader(
                        formatManager.createReaderFor(juce::File(path)));
                    if (reader == nullptr) {
                        mappingError = "A sample pad source could not be opened: " + path;
                        break;
                    }
                    if (std::abs(reader->sampleRate - sampleRate) > 0.5) {
                        mappingError = "A sample pad source sample rate does not match the active audio device: " + path;
                        break;
                    }
                    const auto length = juce::jmin<juce::int64>(
                        reader->lengthInSamples,
                        static_cast<juce::int64>(std::numeric_limits<int>::max()));
                    auto buffer = std::make_shared<juce::AudioBuffer<float>>(reader->numChannels, static_cast<int>(length));
                    if (length <= 0 || buffer->getNumChannels() <= 0 || !reader->read(
                            buffer.get(),
                            0,
                            static_cast<int>(length),
                            0,
                            true,
                            true)) {
                        mappingError = "A sample pad source contains no readable audio: " + path;
                        break;
                    }
                    const auto startMs = static_cast<double>(item.getProperty("startMs", 0.0));
                    const auto endMs = static_cast<double>(item.getProperty("endMs", -1.0));
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
                    if (end <= start || nextPads.find(midiKey) != nextPads.end()) {
                        mappingError = "Sample pad slice is empty or its MIDI key is duplicated.";
                        break;
                    }
                    const auto gainDb = juce::jlimit(-90.0, 24.0, static_cast<double>(item.getProperty("gainDb", 0.0)));
                    nextPads.emplace(midiKey, MidiMonitor::Pad {
                        std::move(buffer),
                        start,
                        end,
                        juce::Decibels::decibelsToGain(static_cast<float>(gainDb)),
                        static_cast<bool>(item.getProperty("loopEnabled", false)),
                    });
                }
            }
            if (mappingError.isNotEmpty()) {
                writeJson(makeError("midi", mappingError));
                continue;
            }
            midiMonitor.replacePads(std::move(nextPads));
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "openMidiInput") {
            const auto name = command.getProperty("name", {}).toString();
            const auto devices = juce::MidiInput::getAvailableDevices();
            const auto device = std::find_if(devices.begin(), devices.end(), [&name](const auto& item) {
                return item.name == name;
            });
            if (device == devices.end()) {
                writeJson(makeError("midi", "The requested MIDI input is no longer available."));
                continue;
            }
            if (midiInput != nullptr) {
                midiInput->stop();
                midiInput.reset();
            }
            midiMonitor.setActive(false);
            midiInput = juce::MidiInput::openDevice(device->identifier, &midiMonitor);
            if (midiInput == nullptr) {
                writeJson(makeError("midi", "Windows could not open the requested MIDI input."));
                continue;
            }
            midiInput->start();
            midiMonitor.setActive(true);
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "closeMidiInput") {
            midiMonitor.setActive(false);
            callback.stopPreview();
            callback.allNotesOff();
            if (midiInput != nullptr) {
                midiInput->stop();
                midiInput.reset();
            }
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
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
                        static_cast<bool>(command.getProperty("loop", false)),
                        previewError,
                        -1))
                        previewError = previewError.isEmpty() ? "Preview range is invalid." : previewError;
                }
            }
            if (previewError.isNotEmpty()) {
                writeJson(makeError("preview", previewError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "stopPreview") {
            callback.stopPreview();
            callback.allNotesOff();
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "setPluginBypassed") {
            rack.setBypassed(static_cast<bool>(command.getProperty("bypassed", true)));
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "setPluginParameter") {
            juce::String parameterError;
            const auto index = static_cast<int>(command.getProperty("index", -1));
            const auto value = static_cast<float>(command.getProperty("value", 0.0));
            if (!rack.setParameter(index, value, parameterError)) {
                writeJson(makeError("plugin", parameterError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "setPluginState") {
            juce::String stateError;
            const auto stateData = command.getProperty("stateData", {}).toString();
            if (!rack.setState(stateData, stateError)) {
                writeJson(makeError("plugin", stateError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "recoverAudioDevice") {
            juce::String midiError;
            if (!midiMonitor.finishRecording(midiError)) {
                writeJson(makeError("recording", midiError));
                continue;
            }
            manager.removeAudioCallback(&callback);
            manager.closeAudioDevice();
            callback.setEmergencyMuted(true);
            const auto recoveryError = initialiseDefaultAudio(manager);
            if (recoveryError.isNotEmpty()) {
                writeJson(makeError("audioDevice", recoveryError));
                continue;
            }
            manager.addAudioCallback(&callback);
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "setAudioDriver") {
            const auto driver = command.getProperty("driver", {}).toString();
            if (driver.isEmpty()) {
                writeJson(makeError("audioDevice", "An audio driver name is required."));
                continue;
            }
            juce::String midiError;
            if (!midiMonitor.finishRecording(midiError)) {
                writeJson(makeError("recording", midiError));
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
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "startRecording") {
            const auto directory = command.getProperty("directory", {}).toString();
            juce::String recordingError;
            if (directory.isEmpty() || !callback.startRecording(juce::File(directory), recordingError)) {
                writeJson(makeError("recording", directory.isEmpty() ? "Recording directory is required." : recordingError));
                continue;
            }
            midiMonitor.beginRecording(juce::File(directory).getChildFile("midi.json"));
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "stopRecording") {
            juce::String recordingError;
            if (!callback.stopRecording(recordingError)) {
                writeJson(makeError("recording", recordingError));
                continue;
            }
            if (!midiMonitor.finishRecording(recordingError)) {
                writeJson(makeError("recording", recordingError));
                continue;
            }
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        if (type == "status") {
            writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            continue;
        }
        writeJson(makeError("protocol", "Unsupported command: " + type));
    }

    callback.setEmergencyMuted(true);
    juce::String ignoredMidiError;
    midiMonitor.finishRecording(ignoredMidiError);
    midiMonitor.setActive(false);
    if (midiInput != nullptr) {
        midiInput->stop();
        midiInput.reset();
    }
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
