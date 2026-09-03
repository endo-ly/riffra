#include "AudioDeviceService.h"

#include <algorithm>
#include <cmath>
#include <memory>
#include <optional>

#include "AudioProtocol.h"
#include "AudioRuntimeStatus.h"
#include "MidiInputService.h"
#include "SafetyAudioCallback.h"
#include "TimelineEngine.h"

namespace riffra {
namespace {

juce::var midiDeviceValue(const juce::MidiDeviceInfo& device) {
    auto* value = new juce::DynamicObject();
    value->setProperty("id", device.identifier);
    value->setProperty("name", device.name);
    return juce::var(value);
}

juce::Array<juce::var> channelNames(const juce::StringArray& names, const bool input) {
    juce::Array<juce::var> channels;
    for (int index = 0; index < names.size(); ++index) {
        auto* channel = new juce::DynamicObject();
        channel->setProperty("index", index);
        channel->setProperty("name", names[index].isNotEmpty() ? names[index]
                                                               : (input ? "Input " : "Output ") +
                                                                     juce::String(index + 1));
        channels.add(juce::var(channel));
    }
    return channels;
}

juce::var listedAudioDevice(const juce::String& name) {
    auto* result = new juce::DynamicObject();
    result->setProperty("name", name);
    result->setProperty("channels", juce::Array<juce::var>{});
    return juce::var(result);
}

std::unique_ptr<juce::XmlElement> configuredAudioXml(const AudioConfiguration& configuration) {
    if (configuration.driver.isEmpty()) return {};
    auto xml = std::make_unique<juce::XmlElement>("DEVICESETUP");
    xml->setAttribute("deviceType", configuration.driver);
    if (configuration.inputDevice.isNotEmpty())
        xml->setAttribute("audioInputDeviceName", configuration.inputDevice);
    if (configuration.outputDevice.isNotEmpty())
        xml->setAttribute("audioOutputDeviceName", configuration.outputDevice);
    return xml;
}

}  // namespace

juce::String AudioDeviceService::accessModeForDriver(const juce::String& driver) {
    if (driver == "Windows Audio" || driver == "Windows Audio (Low Latency Mode)" ||
        driver == "DirectSound")
        return "shared";
    if (driver == "Windows Audio (Exclusive Mode)") return "exclusive";
    return "driverManaged";
}

bool AudioDeviceService::driverRequiresSameDevice(const juce::String& driver) {
    return driver == "ASIO";
}

juce::String AudioDeviceService::defaultDriver() {
#if JUCE_WINDOWS
    return "Windows Audio (Low Latency Mode)";
#elif JUCE_LINUX
    return "ALSA";
#elif JUCE_MAC
    return "CoreAudio";
#else
    return {};
#endif
}

juce::var AudioDeviceService::discover() {
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
        driver->setProperty("devicePairing", sameDevice ? "sameDevice" : "independent");

        juce::Array<juce::var> inputs;
        for (const auto& name : type->getDeviceNames(true)) inputs.add(listedAudioDevice(name));
        driver->setProperty("inputs", inputs);

        juce::Array<juce::var> outputs;
        for (const auto& name : type->getDeviceNames(false)) outputs.add(listedAudioDevice(name));
        driver->setProperty("outputs", outputs);
        driverTypes.add(juce::var(driver));
    }

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "audioDeviceProbe");
    result->setProperty("drivers", driverTypes);
    result->setProperty("emergencyMuted", true);
    result->setProperty("limiterCeiling", 0.98);
    return juce::var(result);
}

std::optional<juce::var> AudioDeviceService::probeDeviceChannels(const juce::String& driver,
                                                                 const juce::String& inputDevice,
                                                                 const juce::String& outputDevice,
                                                                 juce::String& error) {
    juce::AudioDeviceManager manager;
    juce::OwnedArray<juce::AudioIODeviceType> types;
    manager.createAudioDeviceTypes(types);

    if (inputDevice.isEmpty() && outputDevice.isEmpty()) {
        error = "At least one audio device must be selected.";
        return std::nullopt;
    }

    juce::Array<juce::var> inputChannels;
    juce::Array<juce::var> outputChannels;
    bool driverFound = false;
    for (auto* type : types) {
        if (type->getTypeName() != driver) continue;
        driverFound = true;
        type->scanForDevices();
        const auto sameDevice = driverRequiresSameDevice(driver);
        if (sameDevice) {
            auto device =
                std::unique_ptr<juce::AudioIODevice>(type->createDevice(outputDevice, inputDevice));
            if (device == nullptr) {
                error = "The selected audio device could not be opened.";
                return std::nullopt;
            }
            inputChannels = channelNames(device->getInputChannelNames(), true);
            outputChannels = channelNames(device->getOutputChannelNames(), false);
            if (inputChannels.isEmpty() || outputChannels.isEmpty()) {
                error = "The selected audio device returned no channel details.";
                return std::nullopt;
            }
        } else {
            if (inputDevice.isNotEmpty()) {
                auto input = std::unique_ptr<juce::AudioIODevice>(
                    type->createDevice(juce::String{}, inputDevice));
                if (input == nullptr) {
                    error = "The selected input device could not be opened.";
                    return std::nullopt;
                }
                inputChannels = channelNames(input->getInputChannelNames(), true);
                if (inputChannels.isEmpty()) {
                    error = "The selected input device returned no channel details.";
                    return std::nullopt;
                }
            }
            if (outputDevice.isNotEmpty()) {
                auto output = std::unique_ptr<juce::AudioIODevice>(
                    type->createDevice(outputDevice, juce::String{}));
                if (output == nullptr) {
                    error = "The selected output device could not be opened.";
                    return std::nullopt;
                }
                outputChannels = channelNames(output->getOutputChannelNames(), false);
                if (outputChannels.isEmpty()) {
                    error = "The selected output device returned no channel details.";
                    return std::nullopt;
                }
            }
        }
        break;
    }

    if (!driverFound) {
        error = "The selected audio driver could not be found.";
        return std::nullopt;
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

juce::var AudioDeviceService::currentStatus(juce::AudioDeviceManager& manager,
                                            const SafetyAudioCallback& callback,
                                            const MidiMonitor* midi, const juce::String& message,
                                            const TimelineEngine* timeline) {
    auto* status = new juce::DynamicObject();
    status->setProperty("type", "audioStatus");
    const juce::String state =
        callback.isDeviceFaulted() ? "faulted" : (callback.isEmergencyMuted() ? "muted" : "ready");
    status->setProperty("state", state);
    if (callback.isDeviceFaulted())
        status->setProperty(
            "message",
            "Audio device disconnected; output is muted and any captured take is preserved.");
    status->setProperty("emergencyMuted", callback.isEmergencyMuted());
    status->setProperty("masterGainDb", callback.getMasterGainDb());
    status->setProperty("inputPeak", callback.getInputPeak());
    status->setProperty("outputPeak", callback.getOutputPeak());
    status->setProperty("invalidSamples",
                        static_cast<juce::int64>(callback.getInvalidSampleCount()));
    status->setProperty("feedbackSuspected", callback.isFeedbackSuspected());
    status->setProperty("previewing", callback.isPreviewing());
    if (midi != nullptr) {
        status->setProperty("midiInputActive", midi->isActive());
        status->setProperty("midiMessages", static_cast<juce::int64>(midi->getMessageCount()));
        status->setProperty("lastMidiNote", midi->getLastNote());
    }
    status->setProperty("recording", callback.recordingStatus());
    if (timeline != nullptr) {
        const auto timelineStatus = timeline->status();
        status->setProperty("timelineTick", timelineStatus.getProperty("timelineTick", 0));
    }
    if (message.isNotEmpty()) status->setProperty("message", message);

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
        for (int physicalIndex = 0, logicalIndex = 0; physicalIndex < channelNames.size();
             ++physicalIndex) {
            if (!activeInputChannels[physicalIndex]) continue;
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", logicalIndex++);
            channel->setProperty("name", channelNames[physicalIndex].isNotEmpty()
                                             ? channelNames[physicalIndex]
                                             : "Input " + juce::String(physicalIndex + 1));
            inputChannels.add(juce::var(channel));
        }
        status->setProperty("inputChannels", inputChannels);
        juce::Array<juce::var> outputChannels;
        const auto outputChannelNames = device->getOutputChannelNames();
        const auto activeOutputChannels = device->getActiveOutputChannels();
        for (int physicalIndex = 0, logicalIndex = 0; physicalIndex < outputChannelNames.size();
             ++physicalIndex) {
            if (!activeOutputChannels[physicalIndex]) continue;
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", logicalIndex++);
            channel->setProperty("name", outputChannelNames[physicalIndex].isNotEmpty()
                                             ? outputChannelNames[physicalIndex]
                                             : "Output " + juce::String(physicalIndex + 1));
            outputChannels.add(juce::var(channel));
        }
        status->setProperty("outputChannels", outputChannels);
        status->setProperty("sampleRate", device->getCurrentSampleRate());
        status->setProperty("bufferSize", device->getCurrentBufferSizeSamples());
        const auto latencySamples =
            device->getInputLatencyInSamples() + device->getOutputLatencyInSamples();
        const auto latencyMs =
            device->getCurrentSampleRate() > 0.0
                ? 1000.0 * static_cast<double>(latencySamples) / device->getCurrentSampleRate()
                : 0.0;
        status->setProperty("roundTripMs", latencyMs);
    }
    return juce::var(status);
}

juce::var AudioDeviceService::currentMeters(const SafetyAudioCallback& callback) {
    auto* meters = new juce::DynamicObject();
    meters->setProperty("type", "audioMeters");
    meters->setProperty("inputPeak", callback.getInputPeak());
    meters->setProperty("outputPeak", callback.getOutputPeak());
    meters->setProperty("invalidSamples",
                        static_cast<juce::int64>(callback.getInvalidSampleCount()));
    meters->setProperty("emergencyMuted", callback.isEmergencyMuted());
    meters->setProperty("feedbackSuspected", callback.isFeedbackSuspected());
    meters->setProperty("previewing", callback.isPreviewing());
    meters->setProperty("droppedTelemetryFrames",
                        static_cast<juce::int64>(droppedTelemetryCount()));
    meters->setProperty("droppedStateEvents", static_cast<juce::int64>(droppedStateCount()));
    return juce::var(meters);
}

juce::String AudioDeviceService::initialise(juce::AudioDeviceManager& manager,
                                            const AudioConfiguration& configuration) {
    AudioConfiguration resolved = configuration;
    if (resolved.driver.isEmpty()) resolved.driver = defaultDriver();
    const auto& deviceTypes = manager.getAvailableDeviceTypes();
    auto* deviceType = [&]() -> juce::AudioIODeviceType* {
        for (auto* candidate : deviceTypes)
            if (candidate->getTypeName().equalsIgnoreCase(resolved.driver)) return candidate;
        return nullptr;
    }();
    if (deviceType == nullptr)
        return "The requested audio driver is unavailable: " + resolved.driver;

    const auto defaultDeviceName = [deviceType](const bool isInput) {
        const auto names = deviceType->getDeviceNames(isInput);
        if (names.isEmpty()) return juce::String{};
        const auto index =
            juce::jlimit(0, names.size() - 1, deviceType->getDefaultDeviceIndex(isInput));
        return names[index];
    };
    if (resolved.inputDevice.isEmpty()) resolved.inputDevice = defaultDeviceName(true);
    if (resolved.outputDevice.isEmpty()) resolved.outputDevice = defaultDeviceName(false);
    if (driverRequiresSameDevice(resolved.driver)) {
        if (resolved.inputDevice.isEmpty()) resolved.inputDevice = resolved.outputDevice;
        if (resolved.outputDevice.isEmpty()) resolved.outputDevice = resolved.inputDevice;
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
    auto error = manager.initialise(resolved.inputDevice.isNotEmpty() ? 2 : 0, 2, xml.get(), false,
                                    {}, &preferredSetup);
    if (error.isNotEmpty() && configuration.inputDevice.isEmpty()) {
        resolved.inputDevice.clear();
        xml = configuredAudioXml(resolved);
        preferredSetup.inputDeviceName.clear();
        error = manager.initialise(0, 2, xml.get(), false, {}, &preferredSetup);
    }
    if (error.isEmpty() && manager.getCurrentAudioDevice() == nullptr)
        return "The requested audio driver did not open an output device.";
    return error;
}

DeviceFaultWatcher::DeviceFaultWatcher(juce::AudioDeviceManager& manager,
                                       SafetyAudioCallback& callback, TimelineEngine& timeline)
    : deviceManager(manager), audioCallback(callback), timelineEngine(timeline) {}

void DeviceFaultWatcher::changeListenerCallback(juce::ChangeBroadcaster*) {
    const bool present = deviceManager.getCurrentAudioDevice() != nullptr;
    const bool audioActive = !audioCallback.isEmergencyMuted() ||
                             audioCallback.recordingStatus().getProperty("active", false);
    if (!riffra::deviceLossRequiresFault(present, audioActive)) return;
    if (audioCallback.isDeviceFaulted()) return;
    audioCallback.setDeviceFaulted(true);
    audioCallback.setEmergencyMuted(true);
    juce::String ignored;
    timelineEngine.stopRecording();
    audioCallback.stopArrangeRecording(timelineEngine, ignored);
    writeJson(AudioDeviceService::currentStatus(deviceManager, audioCallback));
}

}  // namespace riffra
