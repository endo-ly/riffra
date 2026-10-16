#include "PluginChain.h"

#include <algorithm>
#include <functional>
#include <limits>

namespace riffra {

bool PluginChainAlignment::prepare(const std::vector<juce::String>& deviceIds,
                                   const std::vector<std::pair<juce::String, int>>& requestedDelays,
                                   const int blockSize, juce::String& error) {
    if (blockSize <= 0) {
        error = "Plugin Chain alignment requires a positive block size.";
        return false;
    }
    delays.clear();
    totalDelaySamples = 0;
    for (const auto& [deviceId, samples] : requestedDelays) {
        if (samples <= 0) continue;
        if (std::find(deviceIds.begin(), deviceIds.end(), deviceId) == deviceIds.end()) {
            error = "Plugin Chain main input delay references an unknown device.";
            return false;
        }
        if (samples >= std::numeric_limits<int>::max() - totalDelaySamples) {
            error = "Plugin Chain main input delay exceeds the supported buffer size.";
            return false;
        }
        Delay delay;
        delay.deviceId = deviceId;
        delay.samples = samples;
        delay.ring.setSize(2, samples + 1, false, true, false);
        delay.ring.clear();
        totalDelaySamples += samples;
        delays.push_back(std::move(delay));
    }
    if (!delays.empty()) delayedInputBuffer.setSize(2, blockSize, false, true, false);
    return true;
}

void PluginChainAlignment::reset() noexcept {
    for (auto& delay : delays) {
        delay.ring.clear();
        delay.position = 0;
    }
}

int PluginChainAlignment::delaySamples(const juce::String& deviceId) const noexcept {
    const auto found = std::find_if(delays.begin(), delays.end(),
                                    [&](const Delay& delay) { return delay.deviceId == deviceId; });
    return found != delays.end() ? found->samples : 0;
}

const float* const* PluginChainAlignment::delayDeviceInput(const juce::String& deviceId,
                                                           const float* const* channels,
                                                           const int channelCount,
                                                           const int sampleCount) noexcept {
    const auto found = std::find_if(delays.begin(), delays.end(),
                                    [&](const Delay& delay) { return delay.deviceId == deviceId; });
    if (found == delays.end()) return channels;
    auto& delay = *found;
    const auto ringSize = delay.ring.getNumSamples();
    for (int sample = 0; sample < sampleCount; ++sample) {
        const auto read = (delay.position + 1) % ringSize;
        for (int channel = 0; channel < 2; ++channel) {
            const auto source = channelCount > 1 ? channel : 0;
            delay.ring.setSample(channel, delay.position, channels[source][sample]);
            delayedInputBuffer.setSample(channel, sample, delay.ring.getSample(channel, read));
        }
        delay.position = read;
    }
    return delayedInputBuffer.getArrayOfReadPointers();
}

bool PluginChain::load(const std::vector<PluginDeviceSpec>& values,
                       const std::vector<SidechainSendSpec>& sidechainSends,
                       const double sampleRate, const int blockSize,
                       const PluginProcessingMode mode, juce::String& error,
                       const juce::String& runtimeRole) {
    std::vector<juce::String> deviceIds;
    deviceIds.reserve(values.size());
    for (const auto& value : values) deviceIds.push_back(value.id);
    const auto targets = enabledSidechainTargets(deviceIds, sidechainSends);
    std::vector<Device> candidate;
    std::vector<juce::String> configuredTargets;
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
        const auto capability = rack->status().sidechain;
        Device device{id, std::move(rack), {}};
        device.sidechainCapable = capability.mono || capability.stereo;
        if (sidechain && (capability.mono || capability.stereo) && capability.channels > 0) {
            prepareSidechainInput(device, blockSize);
            configuredTargets.push_back(id);
        }
        candidate.push_back(std::move(device));
    }
    std::sort(configuredTargets.begin(), configuredTargets.end());
    configuredTargets.erase(std::unique(configuredTargets.begin(), configuredTargets.end()),
                            configuredTargets.end());
    devices = std::move(candidate);
    sidechainTargets = std::move(configuredTargets);
    prepareBuffers(blockSize);
    return true;
}

std::vector<juce::String> PluginChain::enabledSidechainTargets(
    const std::vector<juce::String>& deviceIds,
    const std::vector<SidechainSendSpec>& sidechainSends) {
    std::vector<juce::String> targets;
    targets.reserve(sidechainSends.size());
    for (const auto& send : sidechainSends)
        if (send.enabled &&
            std::find(deviceIds.begin(), deviceIds.end(), send.targetDeviceId) != deviceIds.end())
            targets.push_back(send.targetDeviceId);
    std::sort(targets.begin(), targets.end());
    targets.erase(std::unique(targets.begin(), targets.end()), targets.end());
    return targets;
}

bool PluginChain::usesSameSidechainTargets(
    const std::vector<SidechainSendSpec>& sidechainSends) const noexcept {
    std::vector<juce::String> capableDeviceIds;
    capableDeviceIds.reserve(devices.size());
    for (const auto& device : devices)
        if (device.sidechainCapable) capableDeviceIds.push_back(device.id);
    return enabledSidechainTargets(capableDeviceIds, sidechainSends) == sidechainTargets;
}

void PluginChain::prepareSidechainInput(Device& device, const int blockSize) noexcept {
    device.sidechainBuffer.setSize(2, std::max(1, blockSize), false, true, false);
}

void PluginChain::prepareBuffers(const int blockSize) noexcept {
    const auto channels = 2;
    firstBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
    secondBuffer.setSize(channels, std::max(1, blockSize), false, true, false);
}

juce::AudioBuffer<float>* PluginChain::sidechainBuffer(const juce::String& deviceId) noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() && found->sidechainBuffer.getNumChannels() > 0
               ? &found->sidechainBuffer
               : nullptr;
}

const juce::AudioBuffer<float>* PluginChain::sidechainBuffer(
    const juce::String& deviceId) const noexcept {
    const auto found = std::find_if(devices.begin(), devices.end(),
                                    [&](const Device& device) { return device.id == deviceId; });
    return found != devices.end() && found->sidechainBuffer.getNumChannels() > 0
               ? &found->sidechainBuffer
               : nullptr;
}

void PluginChain::clearSidechainBuffers() noexcept {
    for (auto& device : devices) device.sidechainBuffer.clear();
}

void PluginChain::resetSidechainState() noexcept {
    for (auto& device : devices) device.sidechainBuffer.clear();
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
}

void PluginChain::allNotesOff() noexcept {
    for (auto& device : devices) device.rack->allNotesOff();
}

void PluginChain::process(const float* const* inputChannels, const int inputChannelCount,
                          float* const* outputChannels, const int outputChannelCount,
                          const int sampleCount, const juce::MidiBuffer* midi,
                          std::span<PluginParameterAutomation> automation,
                          const std::int64_t timelineSample, const bool playing,
                          PluginChainAlignment* inputAlignment) noexcept {
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
        if (inputAlignment != nullptr && currentInputCount > 0 &&
            inputAlignment->delaySamples(device.id) > 0) {
            deviceInput = inputAlignment->delayDeviceInput(device.id, currentInput,
                                                           currentInputCount, sampleCount);
            deviceInputCount = 2;
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
    for (const auto& device : devices) total += std::max(0, device.rack->latencySamples());
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
