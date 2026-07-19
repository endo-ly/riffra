#include <JuceHeader.h>

#include "SafetyAudioCallback.h"
#include "AudioSafetyDsp.h"
#include "RecordingSelfTest.h"
#include "PluginEditorHost.h"
#include "PluginRack.h"

#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <cmath>
#include <limits>
#include <vector>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <optional>
#include <thread>

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

namespace {

using riffra::SafetyAudioCallback;
using riffra::PluginEditorHost;
using riffra::PluginRack;
using riffra::DCBlocker;
using riffra::FeedbackDetector;

thread_local juce::String currentRequestId;
std::mutex responseMutex;

struct AudioConfiguration {
    juce::String driver;
    juce::String inputDevice;
    juce::String outputDevice;
    int inputChannel = 0;
    double sampleRate = 0.0;
    int bufferSize = 0;
};

juce::String accessModeForDriver(const juce::String& driver) {
    if (driver == "Windows Audio"
        || driver == "Windows Audio (Low Latency Mode)"
        || driver == "DirectSound")
        return "shared";
    if (driver == "Windows Audio (Exclusive Mode)")
        return "exclusive";
    return "driverManaged";
}

bool driverRequiresSameDevice(const juce::String& driver) {
    return driver == "ASIO";
}

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
    const std::lock_guard lock(responseMutex);
    auto response = value;
    if (currentRequestId.isNotEmpty())
        if (auto* object = response.getDynamicObject())
            object->setProperty("requestId", currentRequestId.getLargeIntValue());
    std::cout << juce::JSON::toString(response, true) << std::endl;
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
        driver->setProperty("accessMode", accessModeForDriver(type->getTypeName()));
        driver->setProperty(
            "devicePairing",
            driverRequiresSameDevice(type->getTypeName()) ? "sameDevice" : "independent");

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

juce::var currentStatus(
    juce::AudioDeviceManager& manager,
    const SafetyAudioCallback& callback,
    const PluginRack* rack = nullptr,
    const MidiMonitor* midi = nullptr,
    const juce::String& message = {}) {
    auto* status = new juce::DynamicObject();
    status->setProperty("type", "audioStatus");
    const juce::String state = callback.isDeviceFaulted() ? "faulted"
        : (callback.isEmergencyMuted() ? "muted" : "ready");
    status->setProperty("state", state);
    if (callback.isDeviceFaulted())
        status->setProperty("message", "Audio device disconnected; output is muted and any captured take is preserved.");
    status->setProperty("emergencyMuted", callback.isEmergencyMuted());
    status->setProperty("masterGainDb", callback.getMasterGainDb());
    status->setProperty("inputPeak", callback.getInputPeak());
    status->setProperty("outputPeak", callback.getOutputPeak());
    status->setProperty("invalidSamples", static_cast<juce::int64>(callback.getInvalidSampleCount()));
    status->setProperty("feedbackSuspected", callback.isFeedbackSuspected());
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
    if (message.isNotEmpty())
        status->setProperty("message", message);

    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(device.name);
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(device.name);
    status->setProperty("midiInputs", midiInputs);
    status->setProperty("midiOutputs", midiOutputs);

    if (auto* device = manager.getCurrentAudioDevice()) {
        juce::AudioDeviceManager::AudioDeviceSetup setup;
        manager.getAudioDeviceSetup(setup);
        status->setProperty("driver", device->getTypeName());
        status->setProperty("inputDevice", setup.inputDeviceName);
        status->setProperty("outputDevice", setup.outputDeviceName);
        status->setProperty("inputChannel", callback.getInputChannel());
        juce::Array<juce::var> inputChannels;
        const auto channelNames = device->getInputChannelNames();
        const auto activeInputChannels = device->getActiveInputChannels();
        for (int physicalIndex = 0, logicalIndex = 0;
             physicalIndex < channelNames.size();
             ++physicalIndex) {
            if (!activeInputChannels[physicalIndex])
                continue;
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", logicalIndex++);
            channel->setProperty(
                "name",
                channelNames[physicalIndex].isNotEmpty()
                    ? channelNames[physicalIndex]
                    : "Input " + juce::String(physicalIndex + 1));
            inputChannels.add(juce::var(channel));
        }
        status->setProperty("inputChannels", inputChannels);
        juce::Array<juce::var> outputChannels;
        const auto outputChannelNames = device->getOutputChannelNames();
        const auto activeOutputChannels = device->getActiveOutputChannels();
        for (int physicalIndex = 0, logicalIndex = 0;
             physicalIndex < outputChannelNames.size();
             ++physicalIndex) {
            if (!activeOutputChannels[physicalIndex])
                continue;
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", logicalIndex++);
            channel->setProperty(
                "name",
                outputChannelNames[physicalIndex].isNotEmpty()
                    ? outputChannelNames[physicalIndex]
                    : "Output " + juce::String(physicalIndex + 1));
            outputChannels.add(juce::var(channel));
        }
        status->setProperty("outputChannels", outputChannels);
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

juce::var currentMeters(const SafetyAudioCallback& callback) {
    auto* meters = new juce::DynamicObject();
    meters->setProperty("type", "audioMeters");
    meters->setProperty("inputPeak", callback.getInputPeak());
    meters->setProperty("outputPeak", callback.getOutputPeak());
    meters->setProperty(
        "invalidSamples",
        static_cast<juce::int64>(callback.getInvalidSampleCount()));
    meters->setProperty("feedbackSuspected", callback.isFeedbackSuspected());
    return juce::var(meters);
}

bool parentProcessIsAlive(const std::uint32_t parentPid) noexcept {
#if JUCE_WINDOWS
    const auto process = OpenProcess(SYNCHRONIZE, FALSE, static_cast<DWORD>(parentPid));
    if (process == nullptr)
        return false;
    const auto result = WaitForSingleObject(process, 0);
    CloseHandle(process);
    return result == WAIT_TIMEOUT;
#else
    juce::ignoreUnused(parentPid);
    return true;
#endif
}

std::unique_ptr<juce::XmlElement> configuredAudioXml(const AudioConfiguration& configuration) {
    if (configuration.driver.isEmpty())
        return {};
    auto xml = std::make_unique<juce::XmlElement>("DEVICESETUP");
    xml->setAttribute("deviceType", configuration.driver);
    if (configuration.inputDevice.isNotEmpty())
        xml->setAttribute("audioInputDeviceName", configuration.inputDevice);
    if (configuration.outputDevice.isNotEmpty())
        xml->setAttribute("audioOutputDeviceName", configuration.outputDevice);
    return xml;
}

juce::String initialiseConfiguredAudio(
    juce::AudioDeviceManager& manager,
    const AudioConfiguration& configuration) {
    AudioConfiguration resolved = configuration;
    if (resolved.driver.isEmpty())
        resolved.driver = "Windows Audio (Low Latency Mode)";
    const auto& deviceTypes = manager.getAvailableDeviceTypes();
    auto* deviceType = [&]() -> juce::AudioIODeviceType* {
        for (auto* candidate : deviceTypes)
            if (candidate->getTypeName().equalsIgnoreCase(resolved.driver))
                return candidate;
        return nullptr;
    }();
    if (deviceType == nullptr)
        return "The requested audio driver is unavailable: " + resolved.driver;

    const auto defaultDeviceName = [deviceType](const bool isInput) {
        const auto names = deviceType->getDeviceNames(isInput);
        if (names.isEmpty())
            return juce::String {};
        const auto index = juce::jlimit(
            0,
            names.size() - 1,
            deviceType->getDefaultDeviceIndex(isInput));
        return names[index];
    };
    if (resolved.inputDevice.isEmpty())
        resolved.inputDevice = defaultDeviceName(true);
    if (resolved.outputDevice.isEmpty())
        resolved.outputDevice = defaultDeviceName(false);
    if (driverRequiresSameDevice(resolved.driver)) {
        if (resolved.inputDevice.isEmpty())
            resolved.inputDevice = resolved.outputDevice;
        if (resolved.outputDevice.isEmpty())
            resolved.outputDevice = resolved.inputDevice;
        if (resolved.inputDevice != resolved.outputDevice)
            return "The selected ASIO input and output must use the same device.";
    }
    if (resolved.outputDevice.isEmpty())
        return "The requested audio driver has no output device: " + resolved.driver;

    auto xml = configuredAudioXml(resolved);
    juce::AudioDeviceManager::AudioDeviceSetup preferredSetup;
    preferredSetup.inputDeviceName = resolved.inputDevice;
    preferredSetup.outputDeviceName = resolved.outputDevice;
    preferredSetup.useDefaultInputChannels = true;
    preferredSetup.sampleRate = configuration.sampleRate;
    preferredSetup.bufferSize = configuration.bufferSize;
    auto error = manager.initialise(
        resolved.inputDevice.isNotEmpty() ? 2 : 0,
        2,
        xml.get(),
        false,
        {},
        &preferredSetup);
    if (error.isNotEmpty() && configuration.inputDevice.isEmpty()) {
        resolved.inputDevice.clear();
        xml = configuredAudioXml(resolved);
        preferredSetup.inputDeviceName.clear();
        error = manager.initialise(
            0,
            2,
            xml.get(),
            false,
            {},
            &preferredSetup);
    }
    if (error.isEmpty() && manager.getCurrentAudioDevice() == nullptr)
        return "The requested audio driver did not open an output device.";
    return error;
}

/// Decides whether a device change should fault the engine. We only fault
/// when audio was actually live (playing or recording); a silent/muted device
/// reconfiguration is not a safety event.
inline bool deviceLossRequiresFault(const bool devicePresent, const bool audioActive) {
    return !devicePresent && audioActive;
}

/// Watches the AudioDeviceManager for device loss. JUCE fires a change when a
/// device disappears mid-session; we then mute the engine, mark it faulted, and
/// finalize any in-progress recording so the partial take is preserved.
class DeviceFaultWatcher final : public juce::ChangeListener {
public:
    DeviceFaultWatcher(juce::AudioDeviceManager& manager, SafetyAudioCallback& callback)
        : deviceManager(manager), audioCallback(callback) {}

    void changeListenerCallback(juce::ChangeBroadcaster*) override {
        const bool present = deviceManager.getCurrentAudioDevice() != nullptr;
        const bool audioActive = !audioCallback.isEmergencyMuted()
            || audioCallback.recordingStatus().getProperty("active", false);
        if (!deviceLossRequiresFault(present, audioActive))
            return;
        if (audioCallback.isDeviceFaulted())
            return;
        audioCallback.setDeviceFaulted(true);
        audioCallback.setEmergencyMuted(true);
        juce::String ignored;
        audioCallback.stopRecording(ignored);
        writeJson(currentStatus(deviceManager, audioCallback, nullptr, nullptr));
    }

private:
    juce::AudioDeviceManager& deviceManager;
    SafetyAudioCallback& audioCallback;
};

int serve(
    const std::optional<std::uint32_t> parentPid,
    const AudioConfiguration& startupConfiguration) {
    juce::AudioDeviceManager manager;
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    SafetyAudioCallback callback;
    PluginRack rack;
    PluginEditorHost pluginEditor(rack);
    MidiMonitor midiMonitor;
    std::unique_ptr<juce::MidiInput> midiInput;
    callback.setPluginRack(&rack);
    midiMonitor.setAudioCallback(&callback);
    callback.setEmergencyMuted(true);
    callback.setMasterGainDb(-18.0f);

    auto error = initialiseConfiguredAudio(manager, startupConfiguration);
    juce::String startupMessage;
    if (error.isNotEmpty()) {
        const auto requestedError = error;
        manager.closeAudioDevice();
        AudioConfiguration sharedFallback;
        sharedFallback.driver = "Windows Audio (Low Latency Mode)";
        error = initialiseConfiguredAudio(manager, sharedFallback);
        if (error.isNotEmpty()) {
            manager.closeAudioDevice();
            sharedFallback.driver = "Windows Audio";
            error = initialiseConfiguredAudio(manager, sharedFallback);
        }
        if (error.isNotEmpty()) {
            writeJson(makeError(
                "audioDevice",
                requestedError + ". Shared Windows audio also failed: " + error));
            return 2;
        }
        startupMessage = "The saved audio device was unavailable, so Riffra started with shared Windows audio.";
    }

    auto startupInputChannel = startupMessage.isEmpty()
        ? startupConfiguration.inputChannel
        : 0;
    const auto startupInputChannels = manager.getCurrentAudioDevice() != nullptr
        ? manager.getCurrentAudioDevice()->getActiveInputChannels().countNumberOfSetBits()
        : 0;
    if (startupInputChannel >= startupInputChannels) {
        startupInputChannel = 0;
        startupMessage = "The saved input channel was unavailable, so Input 1 was selected.";
    }
    callback.setInputChannel(startupInputChannel);
    manager.addAudioCallback(&callback);
    DeviceFaultWatcher deviceWatcher(manager, callback);
    manager.addChangeListener(&deviceWatcher);
    writeJson(currentStatus(manager, callback, &rack, &midiMonitor, startupMessage));

    std::atomic<bool> watchdogRunning { true };
    std::thread watchdog;
    if (parentPid.has_value()) {
        watchdog = std::thread([&watchdogRunning, parentPid] {
            while (watchdogRunning.load(std::memory_order_acquire)) {
                std::this_thread::sleep_for(std::chrono::seconds(1));
                if (!watchdogRunning.load(std::memory_order_acquire))
                    break;
                if (!parentProcessIsAlive(*parentPid))
                    std::_Exit(0);
            }
        });
    }

    std::thread commandThread([&] {
        std::string line;
        while (std::getline(std::cin, line)) {
            currentRequestId.clear();
            const auto command = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
            if (!command.isObject()) {
                writeJson(makeError("protocol", "Expected one JSON object per line."));
                continue;
            }

            currentRequestId = command.getProperty("requestId", {}).toString();
            const auto type = command.getProperty("type", {}).toString();
            if (type == "shutdown") break;
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
                const auto blockSize =
                    device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
                if (path.isEmpty()) {
                    writeJson(makeError("pluginPath", "VST3 path is required."));
                    continue;
                }
                if (sampleRate <= 0.0 || blockSize <= 0) {
                    writeJson(makeError("pluginInitialization",
                                        "VST3 loading requires an active audio device."));
                    continue;
                }
                if (const auto pluginError = pluginEditor.load(path, sampleRate, blockSize)) {
                    writeJson(makeError(pluginError->scope, pluginError->message));
                    continue;
                }
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "clearPlugin") {
                juce::String clearError;
                if (!pluginEditor.clear(clearError)) {
                    writeJson(makeError("pluginLifecycle", clearError));
                    continue;
                }
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
                            mappingError =
                                "Each sample pad requires a source path and MIDI key 0-127.";
                            break;
                        }
                        std::unique_ptr<juce::AudioFormatReader> reader(
                            formatManager.createReaderFor(juce::File(path)));
                        if (reader == nullptr) {
                            mappingError = "A sample pad source could not be opened: " + path;
                            break;
                        }
                        if (std::abs(reader->sampleRate - sampleRate) > 0.5) {
                            mappingError =
                                "A sample pad source sample rate does not match the active audio "
                                "device: " +
                                path;
                            break;
                        }
                        const auto length = juce::jmin<juce::int64>(
                            reader->lengthInSamples,
                            static_cast<juce::int64>(std::numeric_limits<int>::max()));
                        auto buffer = std::make_shared<juce::AudioBuffer<float>>(
                            reader->numChannels, static_cast<int>(length));
                        if (length <= 0 || buffer->getNumChannels() <= 0 ||
                            !reader->read(buffer.get(), 0, static_cast<int>(length), 0, true,
                                          true)) {
                            mappingError =
                                "A sample pad source contains no readable audio: " + path;
                            break;
                        }
                        const auto startMs = static_cast<double>(item.getProperty("startMs", 0.0));
                        const auto endMs = static_cast<double>(item.getProperty("endMs", -1.0));
                        const auto start = juce::jlimit(
                            0, static_cast<int>(length),
                            static_cast<int>(std::llround(startMs * reader->sampleRate / 1000.0)));
                        const auto end =
                            endMs <= 0.0 ? static_cast<int>(length)
                                         : juce::jlimit(start + 1, static_cast<int>(length),
                                                        static_cast<int>(std::llround(
                                                            endMs * reader->sampleRate / 1000.0)));
                        if (end <= start || nextPads.find(midiKey) != nextPads.end()) {
                            mappingError =
                                "Sample pad slice is empty or its MIDI key is duplicated.";
                            break;
                        }
                        const auto gainDb = juce::jlimit(
                            -90.0, 24.0, static_cast<double>(item.getProperty("gainDb", 0.0)));
                        nextPads.emplace(
                            midiKey, MidiMonitor::Pad{
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
                const auto device =
                    std::find_if(devices.begin(), devices.end(),
                                 [&name](const auto& item) { return item.name == name; });
                if (device == devices.end()) {
                    writeJson(
                        makeError("midi", "The requested MIDI input is no longer available."));
                    continue;
                }
                if (midiInput != nullptr) {
                    midiInput->stop();
                    midiInput.reset();
                }
                midiMonitor.setActive(false);
                midiInput = juce::MidiInput::openDevice(device->identifier, &midiMonitor);
                if (midiInput == nullptr) {
                    writeJson(
                        makeError("midi", "Windows could not open the requested MIDI input."));
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
                    previewError =
                        "Preview source sample rate does not match the active audio device.";
                } else {
                    const auto length = juce::jmin<juce::int64>(
                        reader->lengthInSamples,
                        static_cast<juce::int64>(std::numeric_limits<int>::max()));
                    juce::AudioBuffer<float> buffer(reader->numChannels, static_cast<int>(length));
                    if (length <= 0 ||
                        !reader->read(&buffer, 0, static_cast<int>(length), 0, true, true)) {
                        previewError = "Preview source contains no readable audio samples.";
                    } else {
                        const auto startMs =
                            static_cast<double>(command.getProperty("startMs", 0.0));
                        const auto endMs = static_cast<double>(command.getProperty("endMs", -1.0));
                        const auto start = juce::jlimit(
                            0, static_cast<int>(length),
                            static_cast<int>(std::llround(startMs * reader->sampleRate / 1000.0)));
                        const auto end =
                            endMs <= 0.0 ? static_cast<int>(length)
                                         : juce::jlimit(start + 1, static_cast<int>(length),
                                                        static_cast<int>(std::llround(
                                                            endMs * reader->sampleRate / 1000.0)));
                        if (!callback.startPreview(
                                buffer, start, end,
                                static_cast<float>(
                                    static_cast<double>(command.getProperty("gain", 1.0))),
                                static_cast<bool>(command.getProperty("loop", false)), previewError,
                                static_cast<int>(command.getProperty("voiceKey", -1))))
                            previewError =
                                previewError.isEmpty() ? "Preview range is invalid." : previewError;
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
            if (type == "stopPreviewForKey") {
                const auto voiceKey = static_cast<int>(command.getProperty("voiceKey", -1));
                callback.stopPreviewForKey(voiceKey);
                callback.stopSynthNote(voiceKey);
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "setPluginBypassed") {
                rack.setBypassed(static_cast<bool>(command.getProperty("bypassed", true)));
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "openPluginEditor") {
                juce::String editorError;
                if (!pluginEditor.open(editorError)) {
                    writeJson(makeError("pluginEditor", editorError));
                    continue;
                }
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
            if (type == "pluginParameterStatus") {
                auto status = currentStatus(manager, callback, &rack, &midiMonitor);
                status.getDynamicObject()->setProperty("plugin", rack.parameterStatus());
                writeJson(status);
                continue;
            }
            if (type == "recoverAudioDevice") {
                juce::String midiError;
                if (!midiMonitor.finishRecording(midiError)) {
                    writeJson(makeError("recording", midiError));
                    continue;
                }
                juce::AudioDeviceManager::AudioDeviceSetup recoverySetup;
                manager.getAudioDeviceSetup(recoverySetup);
                manager.removeAudioCallback(&callback);
                manager.closeAudioDevice();
                callback.setEmergencyMuted(true);
                const auto recoveryError = manager.setAudioDeviceSetup(recoverySetup, true);
                if (recoveryError.isNotEmpty()) {
                    writeJson(makeError("audioDevice", recoveryError));
                    continue;
                }
                manager.addAudioCallback(&callback);
                callback.setDeviceFaulted(false);
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "setAudioDriver") {
                const auto driver = command.getProperty("driver", {}).toString();
                if (driver.isEmpty()) {
                    writeJson(makeError("audioDevice", "An audio driver name is required."));
                    continue;
                }
                AudioConfiguration requested;
                requested.driver = driver;
                requested.inputDevice = command.getProperty("inputDevice", {}).toString();
                requested.outputDevice = command.getProperty("outputDevice", {}).toString();
                requested.inputChannel = static_cast<int>(command.getProperty("inputChannel", 0));
                if (requested.inputChannel < 0) {
                    writeJson(makeError("audioDevice", "Input channel must be zero or greater."));
                    continue;
                }
                requested.sampleRate = static_cast<double>(command.getProperty("sampleRate", 0.0));
                requested.bufferSize = static_cast<int>(command.getProperty("bufferSize", 0));
                juce::String midiError;
                if (!midiMonitor.finishRecording(midiError)) {
                    writeJson(makeError("recording", midiError));
                    continue;
                }
                const auto previousDriver = manager.getCurrentAudioDeviceType();
                const auto previousInputChannel = callback.getInputChannel();
                juce::AudioDeviceManager::AudioDeviceSetup previousSetup;
                manager.getAudioDeviceSetup(previousSetup);
                manager.removeAudioCallback(&callback);
                manager.closeAudioDevice();
                callback.setEmergencyMuted(true);
                const auto restorePreviousDevice = [&]() {
                    manager.closeAudioDevice();
                    AudioConfiguration previous;
                    previous.driver = previousDriver;
                    previous.inputDevice = previousSetup.inputDeviceName;
                    previous.outputDevice = previousSetup.outputDeviceName;
                    previous.inputChannel = previousInputChannel;
                    previous.sampleRate = previousSetup.sampleRate;
                    previous.bufferSize = previousSetup.bufferSize;
                    const auto restoreError = initialiseConfiguredAudio(manager, previous);
                    if (restoreError.isEmpty()) {
                        callback.setInputChannel(previousInputChannel);
                        manager.addAudioCallback(&callback);
                    }
                    return restoreError;
                };
                auto setupError = initialiseConfiguredAudio(manager, requested);
                if (setupError.isNotEmpty()) {
                    const auto restoreError = restorePreviousDevice();
                    writeJson(makeError(
                        "audioDevice",
                        setupError + (restoreError.isEmpty()
                                          ? ". The previous device was restored."
                                          : ". The previous device could not be restored: " +
                                                restoreError)));
                    continue;
                }
                auto* activeDevice = manager.getCurrentAudioDevice();
                const auto activeInputs =
                    activeDevice != nullptr
                        ? activeDevice->getActiveInputChannels().countNumberOfSetBits()
                        : 0;
                if (requested.inputChannel >= activeInputs) {
                    const auto restoreError = restorePreviousDevice();
                    const auto message =
                        juce::String("The selected physical input channel is unavailable.") +
                        (restoreError.isEmpty()
                             ? " The previous device was restored."
                             : " The previous device could not be restored: " + restoreError);
                    writeJson(makeError("audioDevice", message));
                    continue;
                }
                callback.setInputChannel(requested.inputChannel);
                manager.addAudioCallback(&callback);
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "startRecording") {
                const auto directory = command.getProperty("directory", {}).toString();
                juce::String recordingError;
                if (directory.isEmpty() ||
                    !callback.startRecording(juce::File(directory), recordingError)) {
                    writeJson(makeError("recording", directory.isEmpty()
                                                         ? "Recording directory is required."
                                                         : recordingError));
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
            if (type == "meterStatus") {
                writeJson(currentMeters(callback));
                continue;
            }
            writeJson(makeError("protocol", "Unsupported command: " + type));
        }

        juce::MessageManager::callAsync(
            [] { juce::MessageManager::getInstance()->stopDispatchLoop(); });
    });

    juce::MessageManager::getInstance()->runDispatchLoop();
    if (commandThread.joinable()) commandThread.join();
    pluginEditor.close();

    callback.setEmergencyMuted(true);
    juce::String ignoredMidiError;
    midiMonitor.finishRecording(ignoredMidiError);
    midiMonitor.setActive(false);
    if (midiInput != nullptr) {
        midiInput->stop();
        midiInput.reset();
    }
    manager.removeAudioCallback(&callback);
    manager.removeChangeListener(&deviceWatcher);
    manager.closeAudioDevice();
    watchdogRunning.store(false, std::memory_order_release);
    if (watchdog.joinable())
    watchdog.join();
    return 0;
}

juce::var runSafetySelfTest() {
    constexpr double sampleRate = 48000.0;
    constexpr int blockSize = 256;
    constexpr int numChannels = 2;

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "safetySelfTest");
    juce::Array<juce::var> checks;

    {
        DCBlocker blocker;
        blocker.prepare(numChannels);
        std::array<std::array<float, blockSize>, numChannels> buffers {};
        for (auto& buffer : buffers)
            buffer.fill(0.5f);

        std::array<float*, numChannels> channelPtrs {};
        for (int ch = 0; ch < numChannels; ++ch)
            channelPtrs[ch] = buffers[ch].data();

        const int blocks = static_cast<int>(sampleRate * 0.5 / blockSize);
        float lastSample = 0.0f;
        for (int block = 0; block < blocks; ++block) {
            for (auto& buffer : buffers)
                buffer.fill(0.5f);
            blocker.processBlock(channelPtrs.data(), numChannels, blockSize);
            lastSample = buffers[0].back();
        }

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "DCBlocker removes constant offset");
        check->setProperty("inputOffset", 0.5f);
        check->setProperty("outputTail", std::abs(lastSample));
        check->setProperty("passed", std::abs(lastSample) < 0.01f);
        checks.add(juce::var(check));
    }

    {
        DCBlocker blocker;
        blocker.prepare(numChannels);
        std::array<std::array<float, blockSize>, numChannels> buffers {};
        std::array<float*, numChannels> channelPtrs {};
        for (int ch = 0; ch < numChannels; ++ch)
            channelPtrs[ch] = buffers[ch].data();

        constexpr float twoPi = 6.2831853071795864769f;
        constexpr float frequency = 440.0f;
        float phase = 0.0f;
        const float phaseStep = twoPi * frequency / static_cast<float>(sampleRate);
        float maxAbs = 0.0f;
        const int blocks = static_cast<int>(sampleRate * 0.5 / blockSize);
        for (int block = 0; block < blocks; ++block) {
            for (int s = 0; s < blockSize; ++s) {
                buffers[0][s] = std::sin(phase) * 0.5f;
                buffers[1][s] = buffers[0][s];
                phase += phaseStep;
                if (phase >= twoPi)
                    phase -= twoPi;
            }
            blocker.processBlock(channelPtrs.data(), numChannels, blockSize);
            for (int s = blockSize / 2; s < blockSize; ++s)
                maxAbs = std::max(maxAbs, std::abs(buffers[0][s]));
        }

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "DCBlocker preserves audio content");
        check->setProperty("signalAmplitude", 0.5f);
        check->setProperty("preservedAmplitude", maxAbs);
        check->setProperty("passed", maxAbs > 0.3f && maxAbs < 0.6f);
        checks.add(juce::var(check));
    }

    {
        FeedbackDetector detector;
        detector.prepare(sampleRate);
        const int sustainedBlocks = static_cast<int>(
            sampleRate * 300.0 / 1000.0 / blockSize);
        for (int block = 0; block < sustainedBlocks; ++block)
            detector.observe(0.99f, blockSize);

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "FeedbackDetector flags sustained near-peak input");
        check->setProperty("sustainedMs", 300.0);
        check->setProperty("threshold", 0.97f);
        check->setProperty("detected", detector.isSuspected());
        check->setProperty("passed", detector.isSuspected());
        checks.add(juce::var(check));
    }

    {
        FeedbackDetector detector;
        detector.prepare(sampleRate);
        const int shortBlocks = static_cast<int>(
            sampleRate * 50.0 / 1000.0 / blockSize);
        for (int block = 0; block < shortBlocks; ++block)
            detector.observe(0.99f, blockSize);
        detector.observe(0.1f, blockSize);

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "FeedbackDetector does not false-positive on brief peaks");
        check->setProperty("briefMs", 50.0);
        check->setProperty("detected", detector.isSuspected());
        check->setProperty("passed", !detector.isSuspected());
        checks.add(juce::var(check));
    }

    {
        SafetyAudioCallback callback;
        callback.setEmergencyMuted(true);
        std::array<float, blockSize> signal {};
        std::array<float, blockSize> silence {};
        std::array<float, blockSize> output {};
        signal.fill(0.5f);
        const std::array<const float*, 1> signalInput { signal.data() };
        const std::array<const float*, 1> silentInput { silence.data() };
        const std::array<float*, 1> outputs { output.data() };
        const juce::AudioIODeviceCallbackContext context {};
        callback.audioDeviceIOCallbackWithContext(
            signalInput.data(), 1, outputs.data(), 1, blockSize, context);
        callback.audioDeviceIOCallbackWithContext(
            silentInput.data(), 1, outputs.data(), 1, blockSize, context);

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Input meter holds transients until status collection");
        check->setProperty("passed", callback.getInputPeak() >= 0.5f);
        checks.add(juce::var(check));
    }

    {
        PluginRack rack;
        std::array<float, blockSize> mono {};
        std::array<float, blockSize> left {};
        std::array<float, blockSize> right {};
        mono.fill(0.25f);
        const std::array<const float*, 1> inputs { mono.data() };
        const std::array<float*, 2> outputs { left.data(), right.data() };
        rack.process(inputs.data(), 1, outputs.data(), 2, blockSize);

        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Mono input is duplicated to stereo output");
        check->setProperty(
            "passed",
            left.front() == mono.front() && right.front() == mono.front()
                && left.back() == mono.back() && right.back() == mono.back());
        checks.add(juce::var(check));
    }

    for (const auto& check : riffra::runPluginRackSelfTests())
        checks.add(check);

    {
        PluginRack rack;
        const auto status = rack.status();
        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Runtime plugin status excludes persisted state");
        check->setProperty(
            "passed", !status.hasProperty("stateData") && !status.hasProperty("parameters"));
        checks.add(juce::var(check));
    }

    {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Device loss during playback faults the engine");
        check->setProperty("passed", deviceLossRequiresFault(false, true));
        checks.add(juce::var(check));
    }

    {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Device present keeps the engine running");
        check->setProperty("passed", !deviceLossRequiresFault(true, true));
        checks.add(juce::var(check));
    }

    {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Muted idle device reconfiguration is not a fault");
        check->setProperty("passed", !deviceLossRequiresFault(false, false));
        checks.add(juce::var(check));
    }

    {
        // The faulted flag must actually surface as a "faulted" status line so
        // the bridge, UI, and native reconfirmation observe the faulted state.
        SafetyAudioCallback faultedCallback;
        faultedCallback.setDeviceFaulted(true);
        faultedCallback.setEmergencyMuted(true);
        juce::AudioDeviceManager emptyManager;
        const auto status = currentStatus(emptyManager, faultedCallback, nullptr, nullptr);
        const auto* statusObject = status.getDynamicObject();
        const juce::String state =
            statusObject ? statusObject->getProperty("state").toString() : juce::String();
        const juce::String message =
            statusObject ? statusObject->getProperty("message").toString() : juce::String();
        auto* check = new juce::DynamicObject();
        check->setProperty("name", "Device fault is reported as a faulted status");
        check->setProperty(
            "passed",
            state == "faulted" && message.contains("disconnected"));
        checks.add(juce::var(check));
    }

    const bool allPassed = std::all_of(checks.begin(), checks.end(),
        [](const juce::var& check) {
            return static_cast<bool>(check.getProperty("passed", false));
        });
    result->setProperty("checks", checks);
    result->setProperty("passed", allPassed);
    return juce::var(result);
}

} // namespace

int runMain(const juce::StringArray& arguments) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (arguments.size() < 2) {
        writeJson(makeError("arguments", "Use --probe or --serve."));
        return 1;
    }
    const auto command = arguments[1];
    if (command == "--probe") {
        writeJson(probeAudioDevices());
        return 0;
    }
    if (command == "--recording-self-test") {
        if (arguments.size() < 3) {
            writeJson(makeError("arguments", "Use --recording-self-test <directory>."));
            return 1;
        }
        writeJson(riffra::runRecordingSelfTest(juce::File(arguments[2])));
        return 0;
    }
    if (command == "--safety-self-test") {
        writeJson(runSafetySelfTest());
        return 0;
    }
    if (command == "--serve") {
        std::optional<std::uint32_t> parentPid;
        AudioConfiguration configuration;
        for (int index = 2; index < arguments.size(); ++index) {
            const auto argument = arguments[index];
            if (argument != "--parent-pid"
                && argument != "--audio-driver"
                && argument != "--input-device"
                && argument != "--input-channel"
                && argument != "--output-device"
                && argument != "--sample-rate"
                && argument != "--buffer-size")
                continue;
            if (index + 1 >= arguments.size()) {
                writeJson(makeError("arguments", argument + " requires a value."));
                return 1;
            }
            const auto value = arguments[++index];
            if (argument == "--parent-pid") {
                const auto pid = value.getLargeIntValue();
                if (pid <= 0 || pid > std::numeric_limits<std::uint32_t>::max()) {
                    writeJson(makeError("arguments", "--parent-pid must be a positive process id."));
                    return 1;
                }
                parentPid = static_cast<std::uint32_t>(pid);
            } else if (argument == "--audio-driver") {
                configuration.driver = value;
            } else if (argument == "--input-device") {
                configuration.inputDevice = value;
            } else if (argument == "--input-channel") {
                configuration.inputChannel = value.getIntValue();
                if (configuration.inputChannel < 0) {
                    writeJson(makeError("arguments", "--input-channel must be zero or greater."));
                    return 1;
                }
            } else if (argument == "--output-device") {
                configuration.outputDevice = value;
            } else if (argument == "--sample-rate") {
                configuration.sampleRate = value.getDoubleValue();
            } else if (argument == "--buffer-size") {
                configuration.bufferSize = value.getIntValue();
            }
        }
        return serve(parentPid, configuration);
    }
    writeJson(makeError("arguments", "Unknown command: " + command));
    return 1;
}

#if JUCE_WINDOWS
int wmain(int argc, wchar_t* argv[]) {
    juce::StringArray arguments;
    for (int index = 0; index < argc; ++index)
        arguments.add(argv[index]);
    return runMain(arguments);
}
#else
int main(int argc, char* argv[]) {
    juce::StringArray arguments;
    for (int index = 0; index < argc; ++index)
        arguments.add(juce::String::fromUTF8(argv[index]));
    return runMain(arguments);
}
#endif
