#include <JuceHeader.h>

#include "SafetyAudioCallback.h"
#include "AudioRuntimeStatus.h"
#include "FaultInjection.h"
#include "PluginEditorHost.h"
#include "PluginRack.h"
#include "RuntimeLifecycleExecutor.h"
#include "TimelineEngine.h"

#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <cmath>
#include <limits>
#include <vector>
#include <atomic>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <condition_variable>
#include <deque>
#include <exception>
#include <optional>
#include <thread>
#include <unordered_map>

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
using riffra::RuntimeLifecycleExecutor;
using riffra::TimelineEngine;

thread_local juce::String currentRequestId;

enum class OutputKind { control, state, telemetry };

class OutputWriter final {
public:
    OutputWriter() = default;

    ~OutputWriter() { stop(); }

    void enqueue(std::string line, const OutputKind kind) {
        if (kind == OutputKind::control)
            riffra::FaultInjection::stdoutFlood();
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (kind == OutputKind::telemetry
                && telemetryQueue.size() >= kTelemetryQueueLimit) {
                droppedTelemetry.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            if (kind == OutputKind::control)
                controlQueue.push_back(std::move(line));
            else
                telemetryQueue.push_back(std::move(line));
        }
        wake.notify_one();
    }

    void enqueueState(std::string key, std::string line) {
        if (key.empty()) {
            enqueue(std::move(line), OutputKind::telemetry);
            return;
        }
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (const auto existing = stateQueue.find(key); existing != stateQueue.end()) {
                existing->second = std::move(line);
            } else {
                if (stateQueue.size() >= kStateQueueLimit) {
                    droppedState.fetch_add(1, std::memory_order_relaxed);
                    return;
                }
                stateOrder.push_back(key);
                stateQueue.emplace(std::move(key), std::move(line));
            }
        }
        wake.notify_one();
    }

    [[nodiscard]] std::uint64_t droppedTelemetryCount() const noexcept {
        return droppedTelemetry.load(std::memory_order_acquire);
    }

    [[nodiscard]] std::uint64_t droppedStateCount() const noexcept {
        return droppedState.load(std::memory_order_acquire);
    }

private:
    static constexpr std::size_t kTelemetryQueueLimit = 32;
    static constexpr std::size_t kStateQueueLimit = 256;

    void ensureStarted() {
        if (writer.joinable())
            return;
        writer = std::thread([this] { run(); });
    }

    void run() {
        for (;;) {
            std::string line;
            {
                std::unique_lock lock(mutex);
                wake.wait(lock, [this] {
                    return stopping || !controlQueue.empty() || !stateQueue.empty()
                        || !telemetryQueue.empty();
                });
                if (stopping && controlQueue.empty() && stateQueue.empty()
                    && telemetryQueue.empty())
                    return;
                if (!controlQueue.empty()) {
                    line = std::move(controlQueue.front());
                    controlQueue.pop_front();
                } else if (!stateQueue.empty()) {
                    const auto key = std::move(stateOrder.front());
                    stateOrder.pop_front();
                    const auto event = stateQueue.find(key);
                    if (event != stateQueue.end()) {
                        line = std::move(event->second);
                        stateQueue.erase(event);
                    }
                } else {
                    line = std::move(telemetryQueue.front());
                    telemetryQueue.pop_front();
                }
            }
            std::cout << line << '\n' << std::flush;
        }
    }

    void stop() {
        {
            const std::lock_guard lock(mutex);
            stopping = true;
            stateOrder.clear();
            stateQueue.clear();
        }
        wake.notify_one();
        if (writer.joinable())
            writer.join();
    }

    mutable std::mutex mutex;
    std::condition_variable wake;
    std::deque<std::string> controlQueue;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, std::string> stateQueue;
    std::deque<std::string> telemetryQueue;
    std::thread writer;
    std::atomic<std::uint64_t> droppedTelemetry { 0 };
    std::atomic<std::uint64_t> droppedState { 0 };
    bool stopping = false;
};

OutputWriter outputWriter;

juce::var midiDeviceValue(const juce::MidiDeviceInfo& device) {
    auto* value = new juce::DynamicObject();
    value->setProperty("id", device.identifier);
    value->setProperty("name", device.name);
    return juce::var(value);
}

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
        int data1 = 0;
        int data2 = 0;
    };

    void setAudioCallback(SafetyAudioCallback* const callback) noexcept { audioCallback = callback; }
    void setTimelineEngine(TimelineEngine* const engine) noexcept { timelineEngine = engine; }

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
            object->setProperty("data1", event.data1);
            object->setProperty("data2", event.data2);
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

    void handleIncomingMidiMessage(
        juce::MidiInput* source,
        const juce::MidiMessage& message) override {
        messageCount.fetch_add(1, std::memory_order_relaxed);
        const auto status = message.getRawDataSize() > 0
            ? (message.getRawData()[0] & 0xf0)
            : 0;
        if (status >= 0x80 && status <= 0xe0) {
            const juce::ScopedLock lock(recordingLock);
            if (recordingMidi.load(std::memory_order_acquire)
                && recordedEvents.size() < 200'000) {
                recordedEvents.push_back(RecordedEvent {
                    juce::Time::getMillisecondCounterHiRes() - recordingStartMs,
                    status,
                    message.getChannel(),
                    message.getRawDataSize() > 1 ? message.getRawData()[1] : 0,
                    message.getRawDataSize() > 2 ? message.getRawData()[2] : 0,
                });
            }
        }
        const auto routedToTimeline = timelineEngine != nullptr
            && timelineEngine->enqueueLiveMidi(
                message,
                source != nullptr ? source->getIdentifier() : juce::String {});
        if (routedToTimeline) {
            if (message.isNoteOn() || message.isNoteOff())
                lastNote.store(message.getNoteNumber(), std::memory_order_release);
            return;
        }
        if (!message.isNoteOn() && !message.isNoteOff())
            return;

        lastNote.store(message.getNoteNumber(), std::memory_order_release);

        if (audioCallback != nullptr && audioCallback->hasInstrumentPlugin()) {
            audioCallback->enqueuePluginMidi(message);
            return;
        }

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
    TimelineEngine* timelineEngine = nullptr;
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

bool parseMidiBytes(
    const juce::var& value,
    juce::MidiMessage& message,
    juce::String& error) {
    if (!value.isArray()) {
        error = "A bytes array of MIDI data is required.";
        return false;
    }
    const auto bytesArray = *value.getArray();
    if (bytesArray.isEmpty() || bytesArray.size() > 3) {
        error = "MIDI bytes must contain between 1 and 3 bytes.";
        return false;
    }
    std::array<std::uint8_t, 3> bytes{};
    for (int index = 0; index < bytesArray.size(); ++index) {
        const auto& valueAtIndex = bytesArray[index];
        if (!valueAtIndex.isInt() && !valueAtIndex.isInt64() && !valueAtIndex.isDouble()) {
            error = "MIDI bytes must be integer values.";
            return false;
        }
        const auto numeric = static_cast<double>(valueAtIndex);
        if (!std::isfinite(numeric) || std::floor(numeric) != numeric
            || numeric < 0.0 || numeric > 255.0) {
            error = "MIDI bytes must be integers from 0 through 255.";
            return false;
        }
        bytes[static_cast<std::size_t>(index)] = static_cast<std::uint8_t>(numeric);
    }
    if ((bytes[0] & 0x80u) == 0u) {
        error = "The first MIDI byte must be a status byte.";
        return false;
    }
    for (int index = 1; index < bytesArray.size(); ++index) {
        if (bytes[static_cast<std::size_t>(index)] > 0x7fu) {
            error = "MIDI data bytes must be below 128.";
            return false;
        }
    }
    switch (bytesArray.size()) {
    case 1:
        message = juce::MidiMessage(bytes[0]);
        break;
    case 2:
        message = juce::MidiMessage(bytes[0], bytes[1]);
        break;
    default:
        message = juce::MidiMessage(bytes[0], bytes[1], bytes[2]);
        break;
    }
    return true;
}

void writeJson(
    const juce::var& value,
    const juce::String& requestId = {},
    const OutputKind kind = OutputKind::control,
    std::string stateKey = {}) {
    auto response = value;
    const auto effectiveRequestId = requestId.isNotEmpty() ? requestId : currentRequestId;
    if (effectiveRequestId.isNotEmpty())
        if (auto* object = response.getDynamicObject())
            object->setProperty("requestId", effectiveRequestId.getLargeIntValue());
    auto line = juce::JSON::toString(response, true).toStdString();
    if (kind == OutputKind::state)
        outputWriter.enqueueState(std::move(stateKey), std::move(line));
    else
        outputWriter.enqueue(std::move(line), kind);
}

juce::Array<juce::var> channelNames(
    const juce::StringArray& names,
    const bool input) {
    juce::Array<juce::var> channels;
    for (int index = 0; index < names.size(); ++index) {
        auto* channel = new juce::DynamicObject();
        channel->setProperty("index", index);
        channel->setProperty(
            "name",
            names[index].isNotEmpty()
                ? names[index]
                : (input ? "Input " : "Output ") + juce::String(index + 1));
        channels.add(juce::var(channel));
    }
    return channels;
}

juce::var listedAudioDevice(const juce::String& name) {
    auto* result = new juce::DynamicObject();
    result->setProperty("name", name);
    result->setProperty("channels", juce::Array<juce::var> {});
    return juce::var(result);
}

// Passive device discovery: driver and device names only, without opening any
// device. Opening a device (especially an ASIO driver) can reconfigure the
// hardware and interrupt other applications, so startup enumeration stops at
// the name list. Channel details are fetched separately on demand.
juce::var discoverAudioDevices() {
    juce::AudioDeviceManager manager;
    juce::OwnedArray<juce::AudioIODeviceType> types;
    manager.createAudioDeviceTypes(types);

    juce::Array<juce::var> driverTypes;
    for (auto* type : types) {
        type->scanForDevices();
        auto* driver = new juce::DynamicObject();
        driver->setProperty("name", type->getTypeName());
        driver->setProperty("accessMode", accessModeForDriver(type->getTypeName()));
        const auto sameDevice = driverRequiresSameDevice(type->getTypeName());
        driver->setProperty(
            "devicePairing",
            sameDevice ? "sameDevice" : "independent");

        juce::Array<juce::var> inputs;
        for (const auto& name : type->getDeviceNames(true))
            inputs.add(listedAudioDevice(name));
        driver->setProperty("inputs", inputs);

        juce::Array<juce::var> outputs;
        for (const auto& name : type->getDeviceNames(false))
            outputs.add(listedAudioDevice(name));
        driver->setProperty("outputs", outputs);
        driverTypes.add(juce::var(driver));
    }

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "audioDeviceProbe");
    result->setProperty("drivers", driverTypes);
    result->setProperty("emergencyMuted", true);
    result->setProperty("startupGainDb", -18.0);
    result->setProperty("limiterCeiling", 0.98);
    return juce::var(result);
}

juce::var discoverMidiDevices() {
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "midiProbe");
    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(midiDeviceValue(device));
    result->setProperty("midiInputs", midiInputs);
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(midiDeviceValue(device));
    result->setProperty("midiOutputs", midiOutputs);
    return juce::var(result);
}

// Opens a single device to report its channel names. Called only from Audio
// Settings for the device the user has selected, once per device, instead of
// touching every interface during startup. For same-device drivers (ASIO) the
// one open yields both input and output channel names.
juce::var probeDeviceChannels(
    const juce::String& driver,
    const juce::String& inputDevice,
    const juce::String& outputDevice) {
    juce::AudioDeviceManager manager;
    juce::OwnedArray<juce::AudioIODeviceType> types;
    manager.createAudioDeviceTypes(types);

    juce::Array<juce::var> inputChannels;
    juce::Array<juce::var> outputChannels;
    for (auto* type : types) {
        if (type->getTypeName() != driver)
            continue;
        type->scanForDevices();
        const auto sameDevice = driverRequiresSameDevice(driver);
        if (sameDevice) {
            auto device = std::unique_ptr<juce::AudioIODevice>(
                type->createDevice(outputDevice, inputDevice));
            if (device != nullptr) {
                inputChannels = channelNames(device->getInputChannelNames(), true);
                outputChannels = channelNames(device->getOutputChannelNames(), false);
            }
        } else {
            if (inputDevice.isNotEmpty()) {
                auto input = std::unique_ptr<juce::AudioIODevice>(
                    type->createDevice(juce::String {}, inputDevice));
                if (input != nullptr)
                    inputChannels = channelNames(input->getInputChannelNames(), true);
            }
            if (outputDevice.isNotEmpty()) {
                auto output = std::unique_ptr<juce::AudioIODevice>(
                    type->createDevice(outputDevice, juce::String {}));
                if (output != nullptr)
                    outputChannels = channelNames(output->getOutputChannelNames(), false);
            }
        }
        break;
    }

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "deviceChannels");
    result->setProperty("driver", driver);
    result->setProperty("inputDevice", inputDevice);
    result->setProperty("inputChannels", inputChannels);
    result->setProperty("outputDevice", outputDevice);
    result->setProperty("outputChannels", outputChannels);
    return juce::var(result);
}

juce::var currentStatus(
    juce::AudioDeviceManager& manager,
    const SafetyAudioCallback& callback,
    const PluginRack* rack = nullptr,
    const MidiMonitor* midi = nullptr,
    const juce::String& message = {},
    const TimelineEngine* timeline = nullptr) {
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
    if (timeline != nullptr) {
        const auto timelineStatus = timeline->status();
        status->setProperty("timelineTick", timelineStatus.getProperty("timelineTick", 0));
    }
    if (message.isNotEmpty())
        status->setProperty("message", message);

    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(midiDeviceValue(device));
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(midiDeviceValue(device));
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
    meters->setProperty(
        "droppedTelemetryFrames",
        static_cast<juce::int64>(outputWriter.droppedTelemetryCount()));
    meters->setProperty(
        "droppedStateEvents",
        static_cast<juce::int64>(outputWriter.droppedStateCount()));
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

/// Watches the AudioDeviceManager for device loss. JUCE fires a change when a
/// device disappears mid-session; we then mute the engine, mark it faulted, and
/// finalize any in-progress recording so the partial take is preserved.
class DeviceFaultWatcher final : public juce::ChangeListener {
public:
    DeviceFaultWatcher(
        juce::AudioDeviceManager& manager,
        SafetyAudioCallback& callback,
        TimelineEngine& timeline)
        : deviceManager(manager), audioCallback(callback), timelineEngine(timeline) {}

    void changeListenerCallback(juce::ChangeBroadcaster*) override {
        const bool present = deviceManager.getCurrentAudioDevice() != nullptr;
        const bool audioActive = !audioCallback.isEmergencyMuted()
            || audioCallback.recordingStatus().getProperty("active", false);
        if (!riffra::deviceLossRequiresFault(present, audioActive))
            return;
        if (audioCallback.isDeviceFaulted())
            return;
        audioCallback.setDeviceFaulted(true);
        audioCallback.setEmergencyMuted(true);
        juce::String ignored;
        timelineEngine.stopRecording();
        audioCallback.stopArrangeRecording(timelineEngine, ignored);
        audioCallback.stopRecording(ignored);
        writeJson(currentStatus(deviceManager, audioCallback, nullptr, nullptr));
    }

private:
    juce::AudioDeviceManager& deviceManager;
    SafetyAudioCallback& audioCallback;
    TimelineEngine& timelineEngine;
};

int serve(
    const std::optional<std::uint32_t> parentPid,
    const AudioConfiguration& startupConfiguration) {
    juce::AudioDeviceManager manager;
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    TimelineEngine timelineEngine;
    SafetyAudioCallback callback;
    PluginRack rack;
    auto pluginEditor = std::make_shared<PluginEditorHost>(rack);
    std::shared_ptr<PluginEditorHost> trackPluginEditor;
    juce::String trackPluginEditorTrackId;
    juce::String trackPluginEditorDeviceId;
    juce::AudioBuffer<float> comparisonRaw;
    juce::AudioBuffer<float> comparisonProcessed;
    MidiMonitor midiMonitor;
    std::mutex midiInputsLock;
    std::vector<std::unique_ptr<juce::MidiInput>> midiInputs;
    std::atomic<bool> midiListeningEnabled { false };
    std::set<juce::String> activeMidiDeviceIds;
    callback.setPluginRack(&rack);
    callback.setTimelineEngine(&timelineEngine);
    midiMonitor.setAudioCallback(&callback);
    midiMonitor.setTimelineEngine(&timelineEngine);
    callback.setEmergencyMuted(true);
    callback.setMasterGainDb(-18.0f);

    auto reopenAllMidiInputs = [&] {
        const std::lock_guard lock(midiInputsLock);
        for (auto& input : midiInputs) {
            if (input != nullptr) input->stop();
        }
        midiInputs.clear();
        activeMidiDeviceIds.clear();
        if (!midiListeningEnabled.load(std::memory_order_acquire)) return;
        for (const auto& device : juce::MidiInput::getAvailableDevices()) {
            try {
                auto input = juce::MidiInput::openDevice(device.identifier, &midiMonitor);
                if (input == nullptr) continue;
                input->start();
                activeMidiDeviceIds.insert(device.identifier);
                midiInputs.push_back(std::move(input));
            } catch (...) {
                // A single MIDI device that fails to open must not block the others.
            }
        }
    };

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
    DeviceFaultWatcher deviceWatcher(manager, callback, timelineEngine);
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

    std::atomic<bool> midiPollRunning { true };
    std::thread midiPollThread([&] {
        while (midiPollRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
            if (!midiPollRunning.load(std::memory_order_acquire)) break;
            if (!midiListeningEnabled.load(std::memory_order_acquire)) continue;
            std::set<juce::String> currentIds;
            for (const auto& device : juce::MidiInput::getAvailableDevices())
                currentIds.insert(device.identifier);
            bool changed = false;
            {
                const std::lock_guard lock(midiInputsLock);
                changed = currentIds != activeMidiDeviceIds;
            }
            if (changed) {
                reopenAllMidiInputs();
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
            }
        }
    });

    // Meter push thread: periodically writes peak/dropout meters to stdout so
    // the Rust supervisor can emit compact audio-meter events to the frontend without
    // React polling. 50 ms ≈ 20 fps, smooth enough for meter UI.
    std::atomic<bool> meterPushRunning { true };
    std::thread meterPushThread([&] {
        while (meterPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!meterPushRunning.load(std::memory_order_acquire)) break;
            writeJson(currentMeters(callback), {}, OutputKind::telemetry);
        }
    });

    std::atomic<bool> transportPushRunning { true };
    std::thread transportPushThread([&] {
        while (transportPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!transportPushRunning.load(std::memory_order_acquire)) break;
            writeJson(timelineEngine.status(), {}, OutputKind::telemetry);
        }
    });

    // Plugin construction and timeline preparation may execute third-party
    // code for an unbounded amount of time. Keep that work away from the
    // command reader so transport and workspace commands remain serviceable.
    std::atomic<bool> pluginOperationRunning { false };
    std::atomic<bool> timelineOperationRunning { false };
    RuntimeLifecycleExecutor runtimeLifecycle;
    runtimeLifecycle.setTimeoutHandler([] {
        // Do not write to stdout here. The parent may be the stalled party or
        // its pipe may already be back-pressured; the watchdog's only bounded
        // operation is to terminate the isolated process so the Rust
        // supervisor can restart it in emergency-mute state.
        std::_Exit(0);
    });

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
            if (type == "shutdown") {
                callback.setEmergencyMuted(true);
                const auto submitted = runtimeLifecycle.submit(
                    [&] {
                    if (trackPluginEditor != nullptr) {
                        trackPluginEditor->close();
                        trackPluginEditor.reset();
                        trackPluginEditorTrackId.clear();
                        trackPluginEditorDeviceId.clear();
                    }
                    pluginEditor->close();
                    pluginOperationRunning.store(false, std::memory_order_release);
                    timelineOperationRunning.store(false, std::memory_order_release);
                },
                std::chrono::seconds(10));
                if (submitted && !runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
                    std::_Exit(0);
                break;
            }
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
            if (type == "loadTimelineSnapshot" || type == "prepareTimelineSnapshot") {
                if (static_cast<int>(command.getProperty("protocolVersion", 0)) != 1) {
                    writeJson(makeError("timelineProtocol", "Unsupported timeline protocol version."));
                    continue;
                }
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "Another Arrangement Graph is still loading a VST3. The current runtime remains available."));
                    continue;
                }
                const auto commitImmediately = type == "loadTimelineSnapshot";
                auto* device = manager.getCurrentAudioDevice();
                const auto blockSize = device != nullptr
                    ? device->getCurrentBufferSizeSamples()
                    : 0;
                const auto snapshot = command.getProperty("snapshot", {});
                const auto sampleRate = callback.getSampleRate();
                const auto requestId = currentRequestId;
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, snapshot, requestId, sampleRate, blockSize, commitImmediately] {
                        if (commitImmediately && trackPluginEditor != nullptr) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                        }
                        juce::String timelineError;
                        bool loaded = false;
                        try {
                            loaded = timelineEngine.loadSnapshot(
                                snapshot,
                                formatManager,
                                sampleRate,
                                blockSize,
                                timelineError,
                                commitImmediately);
                        } catch (const std::exception& exception) {
                            timelineError =
                                "Arrangement VST3 loading raised an exception: "
                                + juce::String(exception.what());
                        } catch (...) {
                            timelineError =
                                "Arrangement VST3 loading failed with an unknown exception.";
                        }
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!loaded) {
                            writeJson(makeError("timeline", timelineError), requestId);
                        } else {
                            auto* ack = new juce::DynamicObject();
                            ack->setProperty("type", "timelineAck");
                            ack->setProperty("revision", snapshot.getProperty("revision", 0));
                            ack->setProperty("appliedAtAudioClockSample",
                                timelineEngine.status().getProperty("audioClockSample", 0));
                            ack->setProperty(
                                "unavailableClipIds",
                                snapshot.getProperty("unavailableClipIds", juce::Array<juce::var> {}));
                            writeJson(juce::var(ack), requestId);
                        }
                    },
                    std::chrono::seconds(30));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "commitTimelineSnapshot") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3 and cannot be committed yet."));
                    continue;
                }
                const auto requestId = currentRequestId;
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit([&, requestId] {
                    const auto shouldCloseEditor = timelineEngine.hasPreparedSnapshot()
                        && trackPluginEditor != nullptr
                        && !timelineEngine.preparedTrackReusesRuntimeDevices(
                            trackPluginEditorTrackId);
                    if (shouldCloseEditor) {
                        trackPluginEditor->close();
                        trackPluginEditor.reset();
                        trackPluginEditorTrackId.clear();
                        trackPluginEditorDeviceId.clear();
                    }
                    juce::String timelineError;
                    const auto committed = timelineEngine.commitPreparedSnapshot(timelineError);
                    timelineOperationRunning.store(false, std::memory_order_release);
                    if (!committed) {
                        writeJson(makeError("timeline", timelineError), requestId);
                        return;
                    }
                    writeJson(timelineEngine.status(), requestId);
                }, std::chrono::seconds(30));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "discardTimelineSnapshot") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3 and cannot be discarded yet."));
                    continue;
                }
                const auto requestId = currentRequestId;
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit([&, requestId] {
                    timelineEngine.discardPreparedSnapshot();
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(timelineEngine.status(), requestId);
                }, std::chrono::seconds(5));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "setTrackDeviceBypassed") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Track device changes can be retried shortly."));
                    continue;
                }
                const auto requestId = currentRequestId;
                const auto trackId = command.getProperty("trackId", {}).toString();
                const auto deviceId = command.getProperty("deviceId", {}).toString();
                const auto bypassed = static_cast<bool>(command.getProperty("bypassed", false));
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, trackId, deviceId, bypassed] {
                        juce::String deviceError;
                        const auto changed = timelineEngine.setDeviceBypassed(
                            trackId, deviceId, bypassed, deviceError);
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!changed) {
                            writeJson(makeError("trackDevice", deviceError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(10));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "setTrackDeviceParameter") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Track device changes can be retried shortly."));
                    continue;
                }
                const auto requestId = currentRequestId;
                const auto trackId = command.getProperty("trackId", {}).toString();
                const auto deviceId = command.getProperty("deviceId", {}).toString();
                const auto parameterIndex =
                    static_cast<int>(command.getProperty("parameterIndex", -1));
                const auto value = static_cast<float>(command.getProperty("value", 0.0));
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, trackId, deviceId, parameterIndex, value] {
                        juce::String deviceError;
                        const auto changed = timelineEngine.setDeviceParameter(
                            trackId, deviceId, parameterIndex, value, deviceError);
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!changed) {
                            writeJson(makeError("trackDevice", deviceError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(10));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "openTrackPluginEditor") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. The plugin editor can be opened when it finishes."));
                    continue;
                }
                const auto requestId = currentRequestId;
                const auto editorTrackId = command.getProperty("trackId", {}).toString();
                const auto editorDeviceId = command.getProperty("deviceId", {}).toString();
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, editorTrackId, editorDeviceId] {
                        auto* device = timelineEngine.findDevice(editorTrackId, editorDeviceId);
                        if (device == nullptr) {
                            timelineOperationRunning.store(false, std::memory_order_release);
                            writeJson(makeError("trackDevice", "Track Device was not found."), requestId);
                            return;
                        }
                        if (trackPluginEditor != nullptr) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                        }
                        trackPluginEditorTrackId = editorTrackId;
                        trackPluginEditorDeviceId = editorDeviceId;
                        trackPluginEditor = std::make_shared<PluginEditorHost>(
                            *device,
                            [&, editorTrackId, editorDeviceId](const juce::var& state) {
                                const auto stateCopy = state;
                                const auto stateKey =
                                    "track-state:" + (editorTrackId + ":" + editorDeviceId).toStdString();
                                // State events are best-effort latest-value updates. Capacity
                                // drops and shutdown must never become unbounded control errors.
                                (void)runtimeLifecycle.submitState(
                                    stateKey,
                                    [&, editorTrackId, editorDeviceId, stateCopy, stateKey] {
                                        juce::String mirrorError;
                                        if (!timelineEngine.mirrorEditorDeviceState(
                                                editorTrackId,
                                                editorDeviceId,
                                                stateCopy,
                                                mirrorError)) {
                                            writeJson(makeError("trackDevice", mirrorError));
                                        }
                                        auto* changed = new juce::DynamicObject();
                                        changed->setProperty("type", "trackPluginStateChanged");
                                        changed->setProperty("trackId", editorTrackId);
                                        changed->setProperty("deviceId", editorDeviceId);
                                        changed->setProperty(
                                            "parameterValues",
                                            stateCopy.getProperty(
                                                "parameterValues",
                                                juce::Array<juce::var> {}));
                                        changed->setProperty(
                                            "stateData",
                                            stateCopy.getProperty("stateData", {}));
                                        changed->setProperty(
                                            "bypassed",
                                            stateCopy.getProperty("bypassed", false));
                                        writeJson(
                                            juce::var(changed),
                                            {},
                                            OutputKind::state,
                                            stateKey);
                                    },
                                    std::chrono::seconds(10));
                            },
                            [&, editorTrackId, editorDeviceId](const int parameterIndex, const float value) {
                                const auto stateKey = "track-parameter:"
                                    + (editorTrackId + ":" + editorDeviceId).toStdString()
                                    + ":" + std::to_string(parameterIndex);
                                // State events are best-effort latest-value updates. Capacity
                                // drops and shutdown must never become unbounded control errors.
                                (void)runtimeLifecycle.submitState(
                                    stateKey,
                                    [&, editorTrackId, editorDeviceId, parameterIndex, value, stateKey] {
                                        juce::String mirrorError;
                                        if (!timelineEngine.mirrorEditorDeviceParameter(
                                                editorTrackId,
                                                editorDeviceId,
                                                parameterIndex,
                                                value,
                                                mirrorError)) {
                                            writeJson(makeError("trackDevice", mirrorError));
                                            return;
                                        }
                                        auto* changed = new juce::DynamicObject();
                                        changed->setProperty(
                                            "type", "trackPluginParameterChanged");
                                        changed->setProperty("trackId", editorTrackId);
                                        changed->setProperty("deviceId", editorDeviceId);
                                        changed->setProperty("parameterIndex", parameterIndex);
                                        changed->setProperty("value", value);
                                        writeJson(
                                            juce::var(changed),
                                            {},
                                            OutputKind::state,
                                            stateKey);
                                    },
                                    std::chrono::seconds(10));
                            });
                        juce::String editorError;
                        bool opened = false;
                        try {
                            opened = trackPluginEditor->open(editorError);
                        } catch (const std::exception& exception) {
                            editorError =
                                "Track VST3 editor opening raised an exception: "
                                + juce::String(exception.what());
                        } catch (...) {
                            editorError = "Track VST3 editor opening failed with an unknown exception.";
                        }
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!opened) {
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                            writeJson(makeError("pluginEditor", editorError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(30));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "playTimeline") {
                // Processing mode delivery is intentionally nonblocking during
                // navigation. Make Arrange playback self-contained so a Play
                // click cannot race the preceding mode command and leave the
                // TimelineEngine playing while the audio callback still runs
                // in passive/live-input mode (which produces silence).
                callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::arrange);
                timelineEngine.play();
                writeJson(timelineEngine.status());
                continue;
            }
            if (type == "stopTimeline") {
                timelineEngine.stop();
                if (static_cast<bool>(command.getProperty("reportStatus", true)))
                    writeJson(timelineEngine.status());
                continue;
            }
            if (type == "seekTimeline") {
                const auto tick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    command.getProperty("tick", 0)));
                timelineEngine.seekToTick(tick);
                writeJson(timelineEngine.status());
                continue;
            }
            if (type == "setProcessingMode") {
                const auto mode = command.getProperty("mode", {}).toString();
                const auto reportStatus = static_cast<bool>(
                    command.getProperty("reportStatus", true));
                if (mode == "play")
                    callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::play);
                else if (mode == "arrange")
                    callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::arrange);
                else if (mode == "passive")
                    callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::passive);
                else {
                    writeJson(makeError("processingMode", "Processing mode is invalid."));
                    continue;
                }
                if (reportStatus)
                    writeJson(currentStatus(
                        manager, callback, &rack, &midiMonitor, {}, &timelineEngine));
                continue;
            }
            if (type == "probeMidiDevices") {
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "configureSamplePads") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Sample pad changes can be retried shortly."));
                    continue;
                }
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
            if (type == "enableMidiListening") {
                midiListeningEnabled.store(true, std::memory_order_release);
                reopenAllMidiInputs();
                midiMonitor.setActive(true);
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "disableMidiListening") {
                midiListeningEnabled.store(false, std::memory_order_release);
                midiMonitor.setActive(false);
                callback.stopPreview();
                callback.allNotesOff();
                reopenAllMidiInputs();
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "startTakeComparison") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Take comparison can be retried shortly."));
                    continue;
                }
                const auto loadComparisonFile = [&](const juce::String& path,
                                                    const juce::int64 startFrame,
                                                    const juce::int64 endFrame,
                                                    juce::AudioBuffer<float>& target,
                                                    juce::String& loadError) {
                    std::unique_ptr<juce::AudioFormatReader> reader(
                        path.isEmpty() ? nullptr
                                       : formatManager.createReaderFor(juce::File(path)));
                    if (reader == nullptr || reader->lengthInSamples <= 0
                        || reader->lengthInSamples > std::numeric_limits<int>::max()) {
                        loadError = "Take comparison source is unavailable.";
                        return false;
                    }
                    if (startFrame < 0 || endFrame <= startFrame
                        || endFrame > reader->lengthInSamples
                        || endFrame - startFrame > std::numeric_limits<int>::max()) {
                        loadError = "Take comparison range is outside its source.";
                        return false;
                    }
                    const auto sourceFrames = static_cast<int>(endFrame - startFrame);
                    const auto targetRate = callback.getSampleRate();
                    if (targetRate <= 0.0 || reader->sampleRate <= 0.0) {
                        loadError = "Take comparison requires an active output sample rate.";
                        return false;
                    }
                    juce::AudioBuffer<float> source(
                        static_cast<int>(reader->numChannels), sourceFrames + 4);
                    source.clear();
                    if (!reader->read(
                            &source, 0, sourceFrames, startFrame, true, true)) {
                        loadError = "Take comparison source could not be read.";
                        return false;
                    }
                    const auto targetFrames = std::max(
                        1,
                        static_cast<int>(std::llround(
                            static_cast<double>(sourceFrames) * targetRate / reader->sampleRate)));
                    target.setSize(static_cast<int>(reader->numChannels), targetFrames);
                    if (std::abs(reader->sampleRate - targetRate) <= 0.5) {
                        target.copyFrom(0, 0, source, 0, 0, targetFrames);
                        for (int channel = 1; channel < target.getNumChannels(); ++channel)
                            target.copyFrom(channel, 0, source, channel, 0, targetFrames);
                    } else {
                        const auto ratio = reader->sampleRate / targetRate;
                        for (int channel = 0; channel < target.getNumChannels(); ++channel) {
                            juce::LagrangeInterpolator interpolator;
                            interpolator.process(
                                ratio,
                                source.getReadPointer(channel),
                                target.getWritePointer(channel),
                                targetFrames);
                        }
                    }
                    return true;
                };
                juce::String comparisonError;
                if (!loadComparisonFile(
                        command.getProperty("rawPath", {}).toString(),
                        static_cast<juce::int64>(
                            command.getProperty("rawStartFrame", 0)),
                        static_cast<juce::int64>(
                            command.getProperty("rawEndFrame", 0)),
                        comparisonRaw, comparisonError)
                    || !loadComparisonFile(
                        command.getProperty("processedPath", {}).toString(),
                        static_cast<juce::int64>(
                            command.getProperty("processedStartFrame", 0)),
                        static_cast<juce::int64>(
                            command.getProperty("processedEndFrame", 0)),
                        comparisonProcessed, comparisonError)
                    || !callback.startPreview(
                        comparisonRaw, 0, comparisonRaw.getNumSamples(), 1.0f,
                        false, comparisonError, 1)) {
                    writeJson(makeError("takeComparison", comparisonError));
                    continue;
                }
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "switchTakeComparisonVariant") {
                const auto variant = command.getProperty("variant", {}).toString();
                juce::String comparisonError;
                const auto& buffer = variant == "processed"
                    ? comparisonProcessed : comparisonRaw;
                if (!callback.switchPreviewBuffer(1, buffer, comparisonError)) {
                    writeJson(makeError("takeComparison", comparisonError));
                    continue;
                }
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "stopTakeComparison") {
                callback.stopPreviewForKey(1);
                comparisonRaw.setSize(0, 0);
                comparisonProcessed.setSize(0, 0);
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor));
                continue;
            }
            if (type == "previewSample") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Preview can be retried shortly."));
                    continue;
                }
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
if (type == "setPluginState") {
                if (pluginOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "pluginBusy",
                        "The global rack is still loading a VST3. State changes can be retried shortly."));
                    continue;
                }
                const auto stateData = command.getProperty("stateData", {}).toString();
                const auto requestId = currentRequestId;
                pluginOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit([&, requestId, stateData] {
                    juce::String stateError;
                    const auto changed = rack.setState(stateData, stateError);
                    pluginOperationRunning.store(false, std::memory_order_release);
                    if (!changed) {
                        writeJson(makeError("plugin", stateError), requestId);
                        return;
                    }
                    writeJson(currentStatus(manager, callback, &rack, &midiMonitor), requestId);
                },
                std::chrono::seconds(10));
                if (!submitted) {
                    pluginOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
if (type == "sendTrackMidi" || type == "panicTrackMidi") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still changing; targeted MIDI can be retried shortly."));
                    continue;
                }
                const auto trackId = command.getProperty("trackId", {}).toString();
                juce::String timelineError;
                bool accepted = false;
                if (type == "sendTrackMidi") {
                    juce::MidiMessage message;
                    juce::String midiError;
                    if (!parseMidiBytes(command.getProperty("bytes", {}), message, midiError)) {
                        writeJson(makeError("midi", midiError));
                        continue;
                    }
                    accepted = timelineEngine.enqueueTargetedMidi(
                        trackId, message, timelineError);
                } else {
                    accepted = timelineEngine.panicTargetedMidi(trackId, timelineError);
                }
                if (!accepted) {
                    writeJson(makeError("targetedMidi", timelineError));
                    continue;
                }
                writeJson(currentMeters(callback));
                continue;
            }
            if (type == "recoverAudioDevice") {
                if (pluginOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "pluginBusy",
                        "The global rack is still changing a VST3. Audio device recovery can be retried shortly."));
                    continue;
                }
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Audio device recovery can be retried shortly."));
                    continue;
                }
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
                if (pluginOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "pluginBusy",
                        "The global rack is still changing a VST3. Audio driver changes can be retried shortly."));
                    continue;
                }
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError(
                        "timelineBusy",
                        "The Arrangement Graph is still loading a VST3. Audio driver changes can be retried shortly."));
                    continue;
                }
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
            if (type == "startRecording" || type == "startArrangeRecording") {
                const auto directory = command.getProperty("directory", {}).toString();
                const auto allowNoInput = static_cast<bool>(command.getProperty("allowNoInput", false));
                juce::String recordingError;
                const auto started = type == "startArrangeRecording"
                    ? callback.startArrangeRecording(
                        juce::File(directory), timelineEngine, recordingError)
                    : callback.startRecording(
                        juce::File(directory), recordingError, allowNoInput);
                if (directory.isEmpty() || !started) {
                    writeJson(makeError("recording", directory.isEmpty()
                                                         ? "Recording directory is required."
                                                         : recordingError));
                    continue;
                }
                if (type == "startRecording")
                    midiMonitor.beginRecording(juce::File(directory).getChildFile("midi.json"));
                if (type == "startArrangeRecording" &&
                    !timelineEngine.startRecording(
                        static_cast<int>(command.getProperty("countInBeats", 0)),
                        recordingError)) {
                    juce::String rollbackError;
                    (void) callback.stopArrangeRecording(timelineEngine, rollbackError);
                    (void) midiMonitor.finishRecording(rollbackError);
                    writeJson(makeError("recording", recordingError));
                    continue;
                }
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor, {}, &timelineEngine));
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
                writeJson(currentStatus(manager, callback, &rack, &midiMonitor, {}, &timelineEngine));
                continue;
            }
            if (type == "stopArrangeRecording") {
                const auto cancelledCountIn =
                    timelineEngine.cancelRecordingIfCountingIn();

                juce::String recordingError;

                if (cancelledCountIn) {
                    timelineEngine.stop();

                    if (!callback.cancelArrangeRecording(
                            timelineEngine,
                            recordingError)) {
                        writeJson(makeError("recording", recordingError));
                        continue;
                    }
                } else {
                    if (!callback.stopArrangeRecording(
                            timelineEngine,
                            recordingError)) {
                        writeJson(makeError("recording", recordingError));
                        continue;
                    }

                    timelineEngine.stop();
                }

                writeJson(currentStatus(
                    manager,
                    callback,
                    &rack,
                    &midiMonitor,
                    {},
                    &timelineEngine));
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

        const auto cleanupSubmitted = runtimeLifecycle.submit([&] {
            if (trackPluginEditor != nullptr) {
                trackPluginEditor->close();
                trackPluginEditor.reset();
                trackPluginEditorTrackId.clear();
                trackPluginEditorDeviceId.clear();
            }
            pluginEditor->close();
            pluginOperationRunning.store(false, std::memory_order_release);
            timelineOperationRunning.store(false, std::memory_order_release);
        }, std::chrono::seconds(10));
        if (cleanupSubmitted && !runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
            std::_Exit(0);
        juce::MessageManager::callAsync(
            [] { juce::MessageManager::getInstance()->stopDispatchLoop(); });
    });

    juce::MessageManager::getInstance()->runDispatchLoop();
    if (commandThread.joinable()) commandThread.join();
    if (!runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
        std::_Exit(0);
    runtimeLifecycle.requestStop();
    runtimeLifecycle.join();

    callback.setEmergencyMuted(true);
    juce::String ignoredMidiError;
    midiMonitor.finishRecording(ignoredMidiError);
    midiMonitor.setActive(false);
    midiListeningEnabled.store(false, std::memory_order_release);
    reopenAllMidiInputs();
    manager.removeAudioCallback(&callback);
    manager.removeChangeListener(&deviceWatcher);
    manager.closeAudioDevice();
    watchdogRunning.store(false, std::memory_order_release);
    if (watchdog.joinable())
    watchdog.join();
    midiPollRunning.store(false, std::memory_order_release);
    if (midiPollThread.joinable())
        midiPollThread.join();
    meterPushRunning.store(false, std::memory_order_release);
    if (meterPushThread.joinable())
        meterPushThread.join();
    transportPushRunning.store(false, std::memory_order_release);
    if (transportPushThread.joinable())
        transportPushThread.join();
    return 0;
}

} // namespace

int runMain(const juce::StringArray& arguments) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (arguments.size() < 2) {
        writeJson(makeError("arguments", "Use --probe, --probe-midi, --probe-channels or --serve."));
        return 1;
    }
    const auto command = arguments[1];
    if (command == "--probe") {
        writeJson(discoverAudioDevices());
        return 0;
    }
    if (command == "--probe-midi") {
        writeJson(discoverMidiDevices());
        return 0;
    }
    if (command == "--probe-channels") {
        const auto findFlag = [&](const juce::String& flag) -> int {
            return arguments.indexOf(flag, false, 2);
        };
        const auto secondDriver = findFlag("--audio-driver");
        const auto secondInput = findFlag("--input-device");
        const auto secondOutput = findFlag("--output-device");
        const auto readValue = [&](const int flagIndex) -> juce::String {
            if (flagIndex < 2 || flagIndex + 1 >= arguments.size())
                return {};
            return arguments[flagIndex + 1];
        };
        const auto driver = readValue(secondDriver);
        const auto inputDevice = readValue(secondInput);
        const auto outputDevice = readValue(secondOutput);
        if (driver.isEmpty()) {
            writeJson(makeError("arguments", "--probe-channels requires --audio-driver."));
            return 1;
        }
        writeJson(probeDeviceChannels(driver, inputDevice, outputDevice));
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
