#pragma once

#include <cmath>
#include <cstdint>

namespace riffra {

struct TimelineTimebase final {
    std::uint32_t ppq = 960;
    double bpm = 120.0;

    [[nodiscard]] std::int64_t tickToSample(const std::uint64_t tick,
                                            const double sampleRate) const noexcept {
        if (ppq == 0 || !std::isfinite(bpm) || bpm <= 0.0 || !std::isfinite(sampleRate) ||
            sampleRate <= 0.0)
            return 0;
        return static_cast<std::int64_t>(std::llround(static_cast<double>(tick) * sampleRate *
                                                      60.0 / (bpm * static_cast<double>(ppq))));
    }

    [[nodiscard]] std::uint64_t sampleToTick(const std::int64_t sample,
                                             const double sampleRate) const noexcept {
        if (sample <= 0 || ppq == 0 || !std::isfinite(bpm) || bpm <= 0.0 ||
            !std::isfinite(sampleRate) || sampleRate <= 0.0)
            return 0;
        return static_cast<std::uint64_t>(std::llround(
            static_cast<double>(sample) * bpm * static_cast<double>(ppq) / (sampleRate * 60.0)));
    }
};

}  // namespace riffra
