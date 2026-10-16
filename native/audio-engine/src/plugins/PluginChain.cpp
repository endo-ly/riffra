#include "PluginChain.h"

#include <algorithm>
#include <functional>
#include <limits>

namespace riffra {

bool PluginChain::load(const std::vector<PluginDeviceSpec>& values,
                       const std::vector<SidechainSendSpec>& sidechainSends,
                       const double sampleRate, const int blockSize,
                       const PluginProcessingMode mode, juce::String& error,
                       const juce::String& runtimeRole) {
    auto targets = enabledSidechainTargets(sidechainSends);
    std::vector<Device> candidate;
    for (const auto& value : values) {
        const auto& id = value.id;
        const auto& path = value.path;
        if (id.isEmpty() || path.isEmpty()) {
            error = "Plugin Chain devices require an id and path.";
            return false;
        }
        const auto sidechain = std::find(targets.begin(), targets.end(), id) != targets.end();
        auto rack = std::make_unique<PluginRack>();
        if (const auto loadError = rack->load(path, sampleRate, blockSize, mode, sidechain)) {
            error = runtimeRole + " device " + id + " failed at " + loadError->scope + ": " +
                    loadError->message;
            return false;
        }
        if (!rack->applyPersistedState(value.state, error)) {
            error = runtimeRole + " device " + id + " failed at stateApply: " + error;
            return false;
        }
        Device device{id, std::move(rack), {}};
        if (sidechain) prepareSidechainInput(device, blockSize);
        candidate.push_back(std::move(device));
    }
    devices = std::move(candidate);
    sidechainTargets = std::move(targets);
    mainInputDelays.clear();
    prepareBuffers(blockSize);
    return true;
}

std::vector<juce::String> PluginChain::enabledSidechainTargets(
    const std::vector<SidechainSendSpec>& sidechainSends) {
    std::vector<juce::String> targets;
    targets.reserve(sidechainSends.size());
    for (const auto& send : sidechainSends)
        if (send.enabled) targets.push_back(send.targetDeviceId);
    std::sort(targets.begin(), targets.end());
    targets.erase(std::unique(targets.begin(), targets.end()), targets.end());
    return targets;
}

bool PluginChain::usesSameSidechainTargets(
    const std::vector<SidechainSendSpec>& sidechainSends) const noexcept {
    return enabledSidechainTargets(sidechainSends) == sidechainTargets;
}

void PluginChain::prepareSidechainInput(Device& device, const int blockSize) noexcept {
    device.sidechainBuffer.setSize(2, std::max(1, blockSize), false, true, false);
}

void PluginChain::prepareBuffers(const int blockSize) noexcept {
    const auto channels = 2;
    firstBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
    secondBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
    delayedInputBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
}

std::vector<std::pair<juce::String, int>> PluginChain::normalizedMainInputDelays(
    const std::vector<std::pair<juce::String, int>>& delays) {
    std::vector<std::pair<juce::String, int>> normalized;
    normalized.reserve(delays.size());
    for (const auto& [id, samples] : delays)
        if (samples > 0) normalized.emplace_back(id, samples);
    return normalized;
}

bool PluginChain::usesSameMainInputDelays(
    const std::vector<std::pair<juce::String, int>>& delays) const noexcept {
    return normalizedMainInputDelays(delays) == mainInputDelays;
}

bool PluginChain::prepareMainInputDelays(const std::vector<std::pair<juce::String, int>>& delays,
                                         juce::String& error) noexcept {
    const auto normalized = normalizedMainInputDelays(delays);
    if (normalized == mainInputDelays) return true;
    if (!mainInputDelays.empty()) {
        error = "Plugin Chain main input delays differ from the prepared alignment.";
        return false;
    }
    for (const auto& [id, samples] : normalized) {
        const auto found = std::find_if(devices.begin(), devices.end(),
                                        [&](const Device& device) { return device.id == id; });
        if (found == devices.end()) {
            error = "Plugin Chain main input delay references an unknown device.";
            return false;
        }
        found->mainInputDelaySamples = samples;
        found->mainInputDelayBuffer.setSize(2, samples + 1, false, true, false);
        found->mainInputDelayBuffer.clear();
        found->mainInputDelayPosition = 0;
    }
    mainInputDelays = normalized;
    return true;
}

juce::AudioBuffer<float>* PluginChain::sidechainBuffer(const juce::String& deviceId) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() ? &found->sidechainBuffer : nullptr;
}

int PluginChain::mainInputDelay(const juce::String& deviceId) const noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() ? found->mainInputDelaySamples : 0;
}

void PluginChain::clearSidechainBuffers() noexcept {
    for (auto& device : devices) device.sidechainBuffer.clear();
}

void PluginChain::resetSidechainState() noexcept {
    for (auto& device : devices) {
        device.sidechainBuffer.clear();
        device.mainInputDelayBuffer.clear();
        device.mainInputDelayPosition = 0;
    }
}

const float* const* PluginChain::delayDeviceInput(Device& device, const float* const* channels,
                                                  const int channelCount,
                                                  const int sampleCount) noexcept {
    auto& ring = device.mainInputDelayBuffer;
    const auto ringSize = ring.getNumSamples();
    for (int sample = 0; sample < sampleCount; ++sample) {
        const auto read = (device.mainInputDelayPosition + 1) % ringSize;
        for (int channel = 0; channel < 2; ++channel) {
            const auto source = channelCount > 1 ? channel : 0;
            ring.setSample(channel, device.mainInputDelayPosition, channels[source][sample]);
            delayedInputBuffer.setSample(channel, sample, ring.getSample(channel, read));
        }
        device.mainInputDelayPosition = read;
    }
    return delayedInputBuffer.getArrayOfReadPointers();
}

void PluginChain::reset() noexcept {
    for (auto& device : devices) device.rack->reset();
}

void PluginChain::release() noexcept {
    for (auto& device : devices) device.rack->release();
}

void PluginChain::clear() noexcept {
    devices.clear();
    sidechainTargets.clear();
    mainInputDelays.clear();
}

void PluginChain::allNotesOff() noexcept {
    for (auto& device : devices) device.rack->allNotesOff();
}

void PluginChain::process(const float* const* inputChannels, const int inputChannelCount,
                          float* const* outputChannels, const int outputChannelCount,
                          const int sampleCount, const juce::MidiBuffer* midi,
                          std::span<PluginParameterAutomation> automation,
                          const std::int64_t timelineSample, const bool playing) noexcept {
    if (devices.empty()) {
        for (int channel = 0; channel < outputChannelCount; ++channel) {
            if (outputChannels[channel] == nullptr) continue;
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
        auto& device = devices[index];
        auto& target = index % 2 == 0 ? firstBuffer : secondBuffer;
        auto* const* targetChannels = last ? outputChannels : target.getArrayOfWritePointers();
        const auto targetCount = last ? outputChannelCount : target.getNumChannels();
        auto first =
            std::lower_bound(automation.begin(), automation.end(), device.rack.get(),
                             [](const PluginParameterAutomation& binding, const PluginRack* rack) {
                                 return std::less<const PluginRack*>{}(binding.rack, rack);
                             });
        auto lastBinding = first;
        while (lastBinding != automation.end() && lastBinding->rack == device.rack.get())
            ++lastBinding;
        const auto deviceAutomation = std::span<PluginParameterAutomation>(
            first, static_cast<std::size_t>(std::distance(first, lastBinding)));
        const auto auxChannelCount = device.sidechainBuffer.getNumChannels();
        const auto* const* auxChannels =
            auxChannelCount > 0 ? device.sidechainBuffer.getArrayOfReadPointers() : nullptr;
        const auto* const* deviceInput = currentInput;
        auto deviceInputCount = currentInputCount;
        if (device.mainInputDelaySamples > 0 && currentInputCount > 0) {
            deviceInput = delayDeviceInput(device, currentInput, currentInputCount, sampleCount);
            deviceInputCount = delayedInputBuffer.getNumChannels();
        }
        device.rack->process(deviceInput, deviceInputCount, auxChannels, auxChannelCount,
                             targetChannels, targetCount, sampleCount, index == 0 ? midi : nullptr,
                             deviceAutomation, timelineSample, playing);
        currentInput = last ? nullptr : target.getArrayOfReadPointers();
        currentInputCount = targetCount;
    }
}

bool PluginChain::setBypassed(const juce::String& deviceId, const bool bypassed) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    if (found == devices.end()) return false;
    found->rack->setBypassed(bypassed);
    return true;
}

bool PluginChain::setParameter(const juce::String& deviceId, const int parameterIndex,
                               const float value, juce::String& error) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    if (found == devices.end()) {
        error = "Plugin Chain device was not found.";
        return false;
    }
    return found->rack->setParameter(parameterIndex, value, error);
}

bool PluginChain::applyState(const std::vector<PluginDeviceSpec>& values,
                             juce::String& error) noexcept {
    for (const auto& value : values) {
        auto* rack = findDevice(value.id);
        if (rack == nullptr) {
            error = "Plugin Chain state references an unknown device.";
            return false;
        }
        if (!rack->applyPersistedState(value.state, error)) return false;
    }
    return true;
}

std::optional<PluginStateSpec> PluginChain::persistedState(const juce::String& deviceId,
                                                           juce::String& error) const {
    const auto* rack = findDevice(deviceId);
    if (rack == nullptr) {
        error = "Plugin Chain device was not found.";
        return std::nullopt;
    }
    return rack->persistedState(error);
}

int PluginChain::latencySamples() const noexcept {
    std::int64_t total = 0;
    for (const auto& device : devices)
        total +=
            std::max(0, device.rack->latencySamples()) + std::max(0, device.mainInputDelaySamples);
    return static_cast<int>(std::min<std::int64_t>(total, std::numeric_limits<int>::max()));
}

int PluginChain::tailSamples() const noexcept {
    std::int64_t total = 0;
    for (const auto& device : devices) total += std::max(0, device.rack->tailSamples());
    return static_cast<int>(std::min<std::int64_t>(total, std::numeric_limits<int>::max()));
}

int PluginChain::size() const noexcept { return static_cast<int>(devices.size()); }

PluginRack* PluginChain::findDevice(const juce::String& deviceId) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() ? found->rack.get() : nullptr;
}

const PluginRack* PluginChain::findDevice(const juce::String& deviceId) const noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() ? found->rack.get() : nullptr;
}

}  // namespace riffra
