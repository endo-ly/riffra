#pragma once

#include <cmath>
#include <cstdint>

#include "InstrumentPreviewContract.h"

namespace riffra::instrument_preview {

inline double barPositionForFrame(const std::uint64_t frame, const double sampleRate,
                                  const double tempoBpm, const std::uint8_t numerator,
                                  const std::uint8_t denominator) noexcept {
    if (!std::isfinite(sampleRate) || sampleRate <= 0.0 || !isValidTempo(tempoBpm) ||
        numerator == 0 || !isValidDenominator(denominator))
        return 0.0;
    const auto beatPosition = static_cast<double>(frame) / sampleRate * tempoBpm / 60.0;
    const auto beatsPerBar =
        static_cast<double>(numerator) * 4.0 / static_cast<double>(denominator);
    return beatPosition / beatsPerBar;
}

}  // namespace riffra::instrument_preview
