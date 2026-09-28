#include "MidiSourceRegistry.h"

namespace riffra {

std::uint16_t MidiSourceRegistry::indexFor(const juce::String& deviceId) {
    if (deviceId.isEmpty()) return kAllSources;
    const std::lock_guard lock(appendMutex);
    const auto count = published.load(std::memory_order_relaxed);
    for (std::size_t index = 0; index < count; ++index)
        if (deviceIds[index] == deviceId) return static_cast<std::uint16_t>(index);
    if (count == kCapacity) return kUnregistered;
    deviceIds[count] = deviceId;
    published.store(count + 1, std::memory_order_release);
    return static_cast<std::uint16_t>(count);
}

const juce::String& MidiSourceRegistry::deviceId(const std::uint16_t index) const noexcept {
    return index < published.load(std::memory_order_acquire) ? deviceIds[index] : none;
}

}  // namespace riffra
