#pragma once

#include <algorithm>
#include <cmath>
#include <cstdint>

#include "../contract/ExecutionGraph.h"

namespace riffra {

struct TimelineTimebase final {
    std::uint32_t ppq = 960;
    std::vector<TempoChangeSpec> tempoChanges{{0, 120.0}};
    std::vector<TimeSignatureChangeSpec> timeSignatureChanges{{0, 4, 4}};
    [[nodiscard]] double tickToSeconds(const std::uint64_t tick) const noexcept {
        double seconds = 0.0;
        for (std::size_t i = 0; i < tempoChanges.size(); ++i) {
            const auto& point = tempoChanges[i];
            if (point.tick >= tick) break;
            const auto end =
                i + 1 < tempoChanges.size() ? std::min(tick, tempoChanges[i + 1].tick) : tick;
            seconds += static_cast<double>(end - point.tick) * 60.0 /
                       (point.bpm * static_cast<double>(ppq));
        }
        return seconds;
    }

    [[nodiscard]] std::int64_t tickToSample(const std::uint64_t tick,
                                            const double sampleRate) const noexcept {
        return static_cast<std::int64_t>(std::llround(tickToSeconds(tick) * sampleRate));
    }

    [[nodiscard]] std::uint64_t sampleToTick(const std::int64_t sample,
                                             const double sampleRate) const noexcept {
        return static_cast<std::uint64_t>(std::llround(sampleToExactTick(sample, sampleRate)));
    }

    [[nodiscard]] double sampleToExactTick(const std::int64_t sample,
                                           const double sampleRate) const noexcept {
        double remaining = static_cast<double>(std::max<std::int64_t>(0, sample)) / sampleRate;
        for (std::size_t i = 0; i < tempoChanges.size(); ++i) {
            const auto& point = tempoChanges[i];
            const auto ticksPerSecond = point.bpm * static_cast<double>(ppq) / 60.0;
            if (i + 1 < tempoChanges.size()) {
                const auto duration =
                    static_cast<double>(tempoChanges[i + 1].tick - point.tick) / ticksPerSecond;
                if (remaining > duration) {
                    remaining -= duration;
                    continue;
                }
            }
            return static_cast<double>(point.tick) + remaining * ticksPerSecond;
        }
        return 0.0;
    }

    [[nodiscard]] double tempoAt(const std::uint64_t tick) const noexcept {
        const auto next = std::upper_bound(
            tempoChanges.begin(), tempoChanges.end(), tick,
            [](auto position, const auto& point) { return position < point.tick; });
        return (next == tempoChanges.begin() ? tempoChanges.front() : *std::prev(next)).bpm;
    }

    struct MeterPosition final {
        double bar = 0.0;
        std::uint8_t numerator = 4;
        std::uint8_t denominator = 4;
    };

    [[nodiscard]] MeterPosition meterAt(const double tick) const noexcept {
        auto signature = timeSignatureChanges.front();
        double startTick = 0.0;
        double bar = 0.0;
        for (const auto& next : timeSignatureChanges) {
            if (static_cast<double>(next.tick) > tick) break;
            if (next.numerator == signature.numerator && next.denominator == signature.denominator)
                continue;
            const auto barTicks =
                static_cast<double>(ppq) * 4.0 * signature.numerator / signature.denominator;
            bar = std::ceil(bar + (static_cast<double>(next.tick) - startTick) / barTicks);
            startTick = static_cast<double>(next.tick);
            signature = next;
        }
        const auto barTicks =
            static_cast<double>(ppq) * 4.0 * signature.numerator / signature.denominator;
        return {bar + (tick - startTick) / barTicks, signature.numerator, signature.denominator};
    }
};

}  // namespace riffra
