#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <mutex>

namespace riffra {

/// Assigns every MIDI input device a small index that realtime events carry.
///
/// Entries are appended on the control side and never change afterwards, so
/// the audio thread reads a published entry without taking a lock.
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
    /// Any thread. Returns the identifier of a registered index, otherwise empty.
    [[nodiscard]] const juce::String& deviceId(std::uint16_t index) const noexcept;

private:
    std::mutex appendMutex;
    std::array<juce::String, kCapacity> deviceIds;
    std::atomic<std::size_t> published{0};
    const juce::String none;
};

}  // namespace riffra
