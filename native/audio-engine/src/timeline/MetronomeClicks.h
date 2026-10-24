#pragma once

#include <vector>

#include "contract/ExecutionGraph.h"

namespace riffra {

/// Click waveforms of one metronome sound at a graph's output sample rate.
struct MetronomeClicks final {
    /// Played on the first beat of each bar.
    std::vector<float> accent;
    /// Played on every other beat and on each count-in beat.
    std::vector<float> beat;
};

/// Synthesizes the accent and beat clicks of `sound`.
[[nodiscard]] MetronomeClicks renderMetronomeClicks(MetronomeSoundSpec sound, double sampleRate);

}  // namespace riffra
