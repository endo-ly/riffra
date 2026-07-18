#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>
#include <optional>
#include <vector>

namespace riffra {

struct PluginLoadError final {
    juce::String scope;
    juce::String message;
};

class PluginRack final {
public:
    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize);
    void clear() noexcept;
    void release() noexcept;
    void prepare(double sampleRate, int blockSize) noexcept;
    void setBypassed(bool shouldBypass) noexcept;
    bool setParameter(int index, float value, juce::String& error) noexcept;
    bool setState(const juce::String& base64, juce::String& error) noexcept;
    void process(const float* const* inputChannelData, int numInputChannels,
                 float* const* outputChannelData, int numOutputChannels, int numSamples) noexcept;
    [[nodiscard]] juce::var status() const;
    [[nodiscard]] juce::var completeStatus() const;

private:
    friend juce::Array<juce::var> runPluginRackSelfTests();

    struct CachedParameter {
        int index = 0;
        juce::String name;
        float value = 0.0f;
        float defaultValue = 0.0f;
        bool automatable = false;
    };

    void updateParameterCache(juce::AudioProcessor& processor);
    [[nodiscard]] static std::optional<PluginLoadError> configureProcessor(
        juce::AudioProcessor& processor, double sampleRate, int blockSize);
    [[nodiscard]] juce::var cachedStatus() const;

    juce::AudioPluginFormatManager formatManager;
    std::unique_ptr<juce::AudioProcessor> plugin;
    mutable juce::SpinLock pluginLock;
    mutable juce::CriticalSection statusLock;
    std::vector<CachedParameter> cachedParameters;
    juce::String pluginPath;
    juce::String pluginName;
    std::atomic<double> preparedSampleRate{0.0};
    std::atomic<int> preparedBlockSize{0};
    std::atomic<int> pluginInputChannels{0};
    std::atomic<int> pluginOutputChannels{0};
    std::atomic<bool> loaded{false};
    std::atomic<bool> mutationInProgress{false};
    std::atomic<std::uint64_t> bypassedBlocks{0};
    std::atomic<std::uint64_t> contentionBlocks{0};
    std::atomic<std::uint64_t> transitionBlocks{0};
    std::atomic<bool> bypassed{false};
};

[[nodiscard]] juce::Array<juce::var> runPluginRackSelfTests();

}  // namespace riffra
