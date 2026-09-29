#include "MidiSourceRegistry.h"

namespace riffra {

std::uint16_t MidiSourceRegistry::indexFor(const juce::String& deviceId) {
    if (deviceId.isEmpty()) return kAllSources;
    const std::lock_guard lock(appendMutex);
    for (std::size_t index = 0; index < sourceCount; ++index)
        if (deviceIds[index] == deviceId) return static_cast<std::uint16_t>(index);
    if (sourceCount == kCapacity) return kUnregistered;
    deviceIds[sourceCount] = deviceId;
    return static_cast<std::uint16_t>(sourceCount++);
}

std::vector<juce::String> MidiSourceRegistry::snapshot() const {
    const std::lock_guard lock(appendMutex);
    std::vector<juce::String> result;
    result.reserve(sourceCount);
    for (std::size_t index = 0; index < sourceCount; ++index) result.push_back(deviceIds[index]);
    return result;
}

}  // namespace riffra
