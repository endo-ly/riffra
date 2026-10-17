#pragma once

#include <array>
#include <cstdint>
#include <optional>

#include "contract/ExecutionGraph.h"

namespace riffra {

/// Prepared source-to-instrument note mapping shared by timeline and live MIDI.
class MidiNoteTransform final {
public:
    MidiNoteTransform() noexcept {
        for (std::size_t note = 0; note < outputNotes.size(); ++note)
            outputNotes[note] = static_cast<std::int16_t>(note);
    }

    explicit MidiNoteTransform(const InstrumentMidiSpec& settings) noexcept : MidiNoteTransform() {
        for (const auto& keyswitch : settings.keyswitches)
            outputNotes[keyswitch.note] = static_cast<std::int16_t>(keyswitch.note);
        for (std::size_t note = 0; note < outputNotes.size(); ++note) {
            if (isKeyswitch(static_cast<std::uint8_t>(note), settings)) continue;
            const auto output = static_cast<std::int32_t>(note) + settings.transposeSemitones;
            outputNotes[note] =
                output >= 0 && output <= 127 ? static_cast<std::int16_t>(output) : -1;
        }
    }

    [[nodiscard]] std::optional<std::uint8_t> outputNote(const std::uint8_t source) const noexcept {
        const auto output = outputNotes[source];
        if (output < 0 || output > 127) return std::nullopt;
        return static_cast<std::uint8_t>(output);
    }

    bool operator==(const MidiNoteTransform&) const = default;

private:
    [[nodiscard]] static bool isKeyswitch(const std::uint8_t note,
                                          const InstrumentMidiSpec& settings) noexcept {
        for (const auto& keyswitch : settings.keyswitches)
            if (keyswitch.note == note) return true;
        return false;
    }

    std::array<std::int16_t, 128> outputNotes{};
};

}  // namespace riffra
