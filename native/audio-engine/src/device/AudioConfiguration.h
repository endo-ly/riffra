#pragma once

#include <JuceHeader.h>

namespace riffra {

/// Requested audio driver and device setup.
struct AudioConfiguration {
    juce::String driver;
    juce::String inputDevice;
    juce::String outputDevice;
    int inputChannel = 0;
    double sampleRate = 0.0;
    int bufferSize = 0;
};

}  // namespace riffra
