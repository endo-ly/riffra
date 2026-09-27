#pragma once

#include <JuceHeader.h>

#include <cstdint>

#include "contract/ExecutionGraph.h"

namespace riffra {

class OfflineRenderer final {
public:
    struct Result final {
        std::uint64_t frames = 0;
        double sampleRate = 0.0;
    };

    [[nodiscard]] bool render(const OfflineRenderRequestSpec& request,
                              juce::AudioFormatManager& formats, Result& result,
                              juce::String& error);
};

}  // namespace riffra
