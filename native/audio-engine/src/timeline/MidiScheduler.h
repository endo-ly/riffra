#pragma once

#include <JuceHeader.h>

#include <cstddef>
#include <cstdint>
#include <vector>

#include "TimelineTimebase.h"

namespace riffra {

/// Source MIDI data read from the canonical timeline snapshot.
struct MidiNote final {
    std::uint64_t startTick = 0;
    std::uint64_t durationTicks = 1;
    int note = 0;
    int velocity = 0;
    int channel = 1;
};

struct MidiEvent final {
    juce::String kind;
    std::uint64_t tick = 0;
    int channel = 1;
    int data1 = 0;
    int data2 = 0;
};

struct MidiClip final {
    std::uint64_t startTick = 0;
    std::uint64_t durationTicks = 1;
    bool loop = false;
    bool muted = false;
    std::vector<MidiNote> notes;
    std::vector<MidiEvent> events;
};

struct CompiledMidiEvent final {
    std::int64_t sampleOffset = 0;
    int ordering = 0;
    juce::MidiMessage message;
};

struct CompiledMidiClip final {
    std::int64_t startSample = 0;
    std::int64_t lengthSamples = 1;
    bool loop = false;
    bool muted = false;
    std::vector<CompiledMidiEvent> events;
};

/// Compiles timeline MIDI once during graph preparation and schedules only the
/// event range intersecting the current audio block.
class MidiScheduler final {
public:
    using CompiledMidiClip = riffra::CompiledMidiClip;

    static constexpr std::size_t kMaximumMessageBytes = 3;
    static constexpr std::size_t kMidiEventOverhead = sizeof(std::int32_t) + sizeof(std::uint16_t);

    /// Returns the maximum number of compiled events that can intersect a
    /// block of `blockSize` samples for the prepared clips.
    [[nodiscard]] static std::size_t maximumEventsPerBlock(
        const std::vector<CompiledMidiClip>& clips, int blockSize) noexcept;

    /// Prepares a JUCE MIDI buffer for the exact event capacity calculated at
    /// snapshot preparation time.
    [[nodiscard]] static bool prepareBuffer(juce::MidiBuffer& buffer,
                                            std::size_t eventCapacity) noexcept;

    [[nodiscard]] static bool compile(const MidiClip& source, const TimelineTimebase& timebase,
                                      double sampleRate, CompiledMidiClip& destination,
                                      juce::String& error);

    static void schedule(const std::vector<CompiledMidiClip>& clips, std::int64_t rangeStart,
                         int sampleCount, juce::MidiBuffer& destination) noexcept;
};

}  // namespace riffra
