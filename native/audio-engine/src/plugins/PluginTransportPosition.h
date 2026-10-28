#pragma once

#include <cstdint>

namespace riffra {

struct PluginTransportPosition final {
    std::int64_t samplePosition = 0;
    double sampleRate = 0.0;
    double ppqPosition = 0.0;
    double bpm = 120.0;
    double lastBarStartPpq = 0.0;
    int numerator = 4;
    int denominator = 4;
    bool playing = false;
    bool recording = false;
    bool looping = false;
    double loopStartPpq = 0.0;
    double loopEndPpq = 0.0;
};

}  // namespace riffra
