#pragma once

#include <JuceHeader.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <vector>

namespace riffra {

/// Assigns every MIDI input device a small index that realtime events carry.
///
/// Entries are appended on the control side and resolved for recording after capture stops.
class MidiSourceRegistry final {
public:
    /// Index a Track uses to accept every MIDI input device.
    static constexpr std::uint16_t kAllSources = 0xFFFF;
    /// Index of a device that appeared after the table was full. It only
    /// reaches Tracks that accept every device.
    static constexpr std::uint16_t kUnregistered = 0xFFFE;
    static constexpr std::size_t kCapacity = 64;

    /// Control side. Returns the index of a device, registering it on first
    /// use; an empty identifier means every device.
    [[nodiscard]] std::uint16_t indexFor(const juce::String& deviceId);
    /// Copies the registered identifiers for control-side recording finalization.
    [[nodiscard]] std::vector<juce::String> snapshot() const;

private:
    mutable std::mutex appendMutex;
    std::array<juce::String, kCapacity> deviceIds;
    std::size_t sourceCount = 0;
};

}  // namespace riffra
