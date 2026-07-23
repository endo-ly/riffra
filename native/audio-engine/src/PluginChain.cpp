#include "PluginChain.h"

#include <algorithm>

namespace riffra {

bool PluginChain::load(
    const juce::var& values,
    const double sampleRate,
    const int blockSize,
    juce::String& error) {
    if (!values.isArray()) {
        error = "Plugin Chain devices must be an array.";
        return false;
    }
    std::vector<Device> candidate;
    for (const auto& value : *values.getArray()) {
        if (!value.isObject() || value.getProperty("kind", {}).toString() != "plugin" ||
            static_cast<bool>(value.getProperty("disabledPlaceholder", false)))
            continue;
        const auto id = value.getProperty("id", {}).toString();
        const auto path = value.getProperty("path", {}).toString();
        if (id.isEmpty() || path.isEmpty()) {
            error = "Plugin Chain devices require an id and path.";
            return false;
        }
        auto rack = std::make_unique<PluginRack>();
        if (const auto loadError = rack->load(path, sampleRate, blockSize)) {
            error = "Plugin Chain device could not be loaded: " + loadError->message;
            return false;
        }
        const auto state = value.getProperty("stateData", {}).toString();
        if (state.isNotEmpty()) {
            if (!rack->setState(state, error))
                return false;
        } else {
            const auto parameters = value.getProperty("parameterValues", {});
            if (parameters.isArray()) {
                const auto status = rack->parameterStatus().getProperty("parameters", {});
                const auto count = status.isArray() ? status.size() : 0;
                for (int index = 0; index < std::min(parameters.size(), count); ++index)
                    if (!rack->setParameter(index, static_cast<float>(parameters[index]), error))
                        return false;
            }
        }
        rack->setBypassed(static_cast<bool>(value.getProperty("bypassed", false)));
        candidate.push_back(Device { id, std::move(rack) });
    }
    devices = std::move(candidate);
    prepare(sampleRate, blockSize);
    return true;
}

void PluginChain::prepare(const double sampleRate, const int blockSize) noexcept {
    const auto channels = 2;
    firstBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
    secondBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
    for (auto& device : devices)
        device.rack->prepare(sampleRate, blockSize);
}

void PluginChain::release() noexcept {
    for (auto& device : devices)
        device.rack->release();
}

void PluginChain::clear() noexcept {
    devices.clear();
}

void PluginChain::allNotesOff() noexcept {
    for (auto& device : devices)
        device.rack->allNotesOff();
}

void PluginChain::process(
    const float* const* inputChannels,
    const int inputChannelCount,
    float* const* outputChannels,
    const int outputChannelCount,
    const int sampleCount,
    const juce::MidiBuffer* midi) noexcept {
    if (devices.empty()) {
        for (int channel = 0; channel < outputChannelCount; ++channel) {
            if (outputChannels[channel] == nullptr)
                continue;
            const auto source = inputChannelCount > 0
                ? inputChannels[std::min(channel, inputChannelCount - 1)]
                : nullptr;
            if (source != nullptr)
                juce::FloatVectorOperations::copy(outputChannels[channel], source, sampleCount);
            else
                juce::FloatVectorOperations::clear(outputChannels[channel], sampleCount);
        }
        return;
    }
    const float* const* currentInput = inputChannels;
    auto currentInputCount = inputChannelCount;
    for (std::size_t index = 0; index < devices.size(); ++index) {
        const auto last = index + 1 == devices.size();
        auto& target = index % 2 == 0 ? firstBuffer : secondBuffer;
        auto* const* targetChannels = last ? outputChannels : target.getArrayOfWritePointers();
        const auto targetCount = last ? outputChannelCount : target.getNumChannels();
        devices[index].rack->process(
            currentInput,
            currentInputCount,
            targetChannels,
            targetCount,
            sampleCount,
            index == 0 ? midi : nullptr);
        currentInput = last ? nullptr : target.getArrayOfReadPointers();
        currentInputCount = targetCount;
    }
}

bool PluginChain::setBypassed(const juce::String& deviceId, const bool bypassed) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(), [&](const Device& device) {
        return device.id == deviceId;
    });
    if (found == devices.end())
        return false;
    found->rack->setBypassed(bypassed);
    return true;
}

bool PluginChain::setParameter(
    const juce::String& deviceId,
    const int parameterIndex,
    const float value,
    juce::String& error) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(), [&](const Device& device) {
        return device.id == deviceId;
    });
    if (found == devices.end()) {
        error = "Plugin Chain device was not found.";
        return false;
    }
    return found->rack->setParameter(parameterIndex, value, error);
}

int PluginChain::latencySamples() const noexcept {
    auto total = 0;
    for (const auto& device : devices)
        total += std::max(0, device.rack->latencySamples());
    return total;
}

int PluginChain::size() const noexcept {
    return static_cast<int>(devices.size());
}

PluginRack* PluginChain::findDevice(const juce::String& deviceId) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(), [&](const Device& device) {
        return device.id == deviceId;
    });
    return found != devices.end() ? found->rack.get() : nullptr;
}

} // namespace riffra
