#include "AudioDeviceService.h"

#include <algorithm>
#include <cmath>
#include <memory>
#include <optional>

namespace riffra {
namespace {

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
    result->setProperty("refreshedAtMs", juce::Time::currentTimeMillis());
    result->setProperty("message", "Audio device list refreshed.");
    result->setProperty("muteReasons", 0);
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
    preferredSetup.useDefaultInputChannels = resolved.inputDevice.isEmpty();
    if (!preferredSetup.useDefaultInputChannels) {
        preferredSetup.inputChannels.clear();
        preferredSetup.inputChannels.setBit(std::max(0, resolved.inputChannel));
    }
    preferredSetup.sampleRate = configuration.sampleRate;
    preferredSetup.bufferSize = configuration.bufferSize;
    const auto error = manager.initialise(resolved.inputDevice.isNotEmpty() ? 2 : 0, 2, xml.get(),
                                          false, {}, &preferredSetup);
    if (error.isEmpty() && manager.getCurrentAudioDevice() == nullptr)
        return "The requested audio driver did not open an output device.";
    return error;
}

}  // namespace riffra
