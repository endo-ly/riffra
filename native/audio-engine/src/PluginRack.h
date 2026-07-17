#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>
#include <vector>

namespace riffra {

class PluginRack final {
public:
    bool load(const juce::String& path, double sampleRate, int blockSize, juce::String& error);
    void clear() noexcept;
    void release() noexcept;
    void prepare(double sampleRate, int blockSize) noexcept;
    void setBypassed(bool shouldBypass) noexcept;
    bool setParameter(int index, float value, juce::String& error) noexcept;
    bool setState(const juce::String& base64, juce::String& error) noexcept;
    void process(
        const float* const* inputChannelData,
        int numInputChannels,
        float* const* outputChannelData,
        int numOutputChannels,
        int numSamples) noexcept;
    [[nodiscard]] juce::var status() const;
    [[nodiscard]] juce::var completeStatus() const;

private:
    struct CachedParameter {
        int index = 0;
        juce::String name;
        float value = 0.0f;
        float defaultValue = 0.0f;
        bool automatable = false;
    };

    void updateParameterCache(juce::AudioProcessor& processor);
    [[nodiscard]] juce::var cachedStatus() const;

    juce::AudioPluginFormatManager formatManager;
    std::unique_ptr<juce::AudioPluginInstance> plugin;
    mutable juce::SpinLock pluginLock;
    mutable juce::CriticalSection statusLock;
    std::vector<CachedParameter> cachedParameters;
    juce::String pluginPath;
    juce::String pluginName;
    std::atomic<double> preparedSampleRate { 0.0 };
    std::atomic<int> preparedBlockSize { 0 };
    std::atomic<bool> loaded { false };
    std::atomic<bool> mutationInProgress { false };
    std::atomic<std::uint64_t> bypassedBlocks { 0 };
    std::atomic<std::uint64_t> contentionBlocks { 0 };
    std::atomic<std::uint64_t> transitionBlocks { 0 };
    std::atomic<bool> bypassed { false };
};

} // namespace riffra
