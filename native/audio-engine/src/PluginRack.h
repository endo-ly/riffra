#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>

namespace riffra {

class PluginRack final {
public:
    bool load(const juce::String& path, double sampleRate, int blockSize, juce::String& error);
    void clear() noexcept;
    void prepare(double sampleRate, int blockSize) noexcept;
    void setBypassed(bool shouldBypass) noexcept;
    void process(
        const float* const* inputChannelData,
        int numInputChannels,
        float* const* outputChannelData,
        int numOutputChannels,
        int numSamples) noexcept;
    [[nodiscard]] juce::var status() const;

private:
    juce::AudioPluginFormatManager formatManager;
    std::unique_ptr<juce::AudioPluginInstance> plugin;
    mutable juce::SpinLock pluginLock;
    double preparedSampleRate = 0.0;
    int preparedBlockSize = 0;
    juce::String pluginPath;
    juce::String pluginName;
    std::atomic<std::uint64_t> bypassedBlocks { 0 };
    std::atomic<bool> bypassed { false };
};

} // namespace riffra
