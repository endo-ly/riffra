#pragma once

#include <JuceHeader.h>

#include <cstdint>

namespace riffra {

class OfflineRenderer final {
public:
    struct Result final {
        std::uint64_t frames = 0;
        double sampleRate = 0.0;
    };

    [[nodiscard]] bool render(
        const juce::var& snapshot,
        juce::AudioFormatManager& formats,
        const juce::File& destination,
        std::uint64_t startTick,
        std::uint64_t endTick,
        double sampleRate,
        int blockSize,
        float masterGainDb,
        bool normalize,
        Result& result,
        juce::String& error);
};

} // namespace riffra
