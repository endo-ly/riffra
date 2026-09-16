#pragma once

#include <cmath>
#include <cstddef>
#include <cstdint>

namespace riffra::instrument_preview {

inline constexpr double kMinimumTempoBpm = 30.0;
inline constexpr double kMaximumTempoBpm = 300.0;
inline constexpr std::uint16_t kMinimumTicksPerBeat = 1;
inline constexpr std::uint16_t kMaximumTicksPerBeat = 32'767;
inline constexpr std::uint8_t kMaximumTimeSignatureDenominator = 128;
inline constexpr std::size_t kMinimumNoteCount = 1;
inline constexpr std::size_t kMaximumNoteCount = 32;
inline constexpr double kMaximumDurationSeconds = 10.0;
inline constexpr std::uint8_t kMaximumMidiValue = 127;

inline bool isValidTempo(const double tempoBpm) noexcept {
    return std::isfinite(tempoBpm) && tempoBpm >= kMinimumTempoBpm && tempoBpm <= kMaximumTempoBpm;
}

inline bool isValidTicksPerBeat(const std::uint16_t ticksPerBeat) noexcept {
    return ticksPerBeat >= kMinimumTicksPerBeat && ticksPerBeat <= kMaximumTicksPerBeat;
}

inline bool isValidDenominator(const std::uint8_t denominator) noexcept {
    return denominator > 0 && denominator <= kMaximumTimeSignatureDenominator &&
           (denominator & static_cast<std::uint8_t>(denominator - 1)) == 0;
}

inline bool isWithinDurationLimit(const double tempoBpm, const std::uint16_t ticksPerBeat,
                                  const std::uint64_t lengthTicks) noexcept {
    if (!isValidTempo(tempoBpm) || !isValidTicksPerBeat(ticksPerBeat) || lengthTicks == 0)
        return false;
    const auto durationSeconds = static_cast<long double>(lengthTicks) * 60.0L /
                                 (static_cast<long double>(ticksPerBeat) * tempoBpm);
    return std::isfinite(static_cast<double>(durationSeconds)) &&
           durationSeconds <= static_cast<long double>(kMaximumDurationSeconds);
}

}  // namespace riffra::instrument_preview
