#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>
#include <optional>
#include <vector>

namespace riffra {

class PluginEditorHost;
class PluginRackTestPeer;

struct PluginLoadError final {
    juce::String scope;
    juce::String message;
};

class PluginRack final {
public:
    ~PluginRack();

    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize);
    void clear() noexcept;
    void release() noexcept;
    void prepare(double sampleRate, int blockSize) noexcept;
    void reset() noexcept;
    void setBypassed(bool shouldBypass) noexcept;
    bool setParameter(int index, float value, juce::String& error) noexcept;
    bool setState(const juce::String& base64, juce::String& error) noexcept;
    bool applyPersistedState(const juce::var& state, juce::String& error) noexcept;
    [[nodiscard]] juce::var persistedState(juce::String& error) const;
    void process(const float* const* inputChannelData, int numInputChannels,
                 float* const* outputChannelData, int numOutputChannels, int numSamples,
                 const juce::MidiBuffer* timelineMidi = nullptr) noexcept;
    void enqueueMidi(const juce::MidiMessage& message) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isLoaded() const noexcept;
    [[nodiscard]] bool isInstrument() const noexcept;
    [[nodiscard]] int latencySamples() const noexcept;
    [[nodiscard]] int tailSamples() const noexcept;
    [[nodiscard]] juce::var status() const;
    [[nodiscard]] juce::var parameterStatus() const;
    [[nodiscard]] std::size_t parameterCount() const noexcept;
    void addProcessorListener(juce::AudioProcessorListener& listener) noexcept;
    void removeProcessorListener(juce::AudioProcessorListener& listener) noexcept;
    /// Queues a live-only editor parameter change. It is applied by `process`
    /// while that rack already owns its plugin lock at a block boundary.
    void enqueueParameterChange(int index, float value) noexcept;

private:
    friend class PluginEditorHost;
    friend class PluginRackTestPeer;

    struct CachedParameter {
        int index = 0;
        juce::String name;
        float value = 0.0f;
        float defaultValue = 0.0f;
        bool automatable = false;
    };

    void updateParameterCache(juce::AudioProcessor& processor);
    [[nodiscard]] juce::AudioProcessorEditor* createEditor(juce::String& error);
    [[nodiscard]] juce::String currentPluginName() const;
    [[nodiscard]] static std::optional<PluginLoadError> configureProcessor(
        juce::AudioProcessor& processor, double sampleRate, int blockSize);
    [[nodiscard]] juce::var cachedStatus(bool includeParameters) const;
    void applyQueuedParameterChanges() noexcept;
    bool allocateParameterQueue(std::size_t count, juce::String& error) noexcept;

    class PendingMidi final {
    public:
        void reset();
        void add(const juce::MidiMessage& message);
        void appendTo(juce::MidiBuffer& destination, int sampleCount);

    private:
        juce::CriticalSection lock;
        juce::MidiBuffer messages;
    };

    juce::AudioPluginFormatManager formatManager;
    std::unique_ptr<juce::AudioProcessor> plugin;
    PendingMidi pendingMidi;
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
    std::atomic<std::uint64_t> processedBlocks{0};
    std::atomic<std::uint64_t> contentionBlocks{0};
    std::atomic<std::uint64_t> transitionBlocks{0};
    std::atomic<std::uint64_t> loadCount{0};
    std::atomic<std::uint64_t> destroyCount{0};
    std::atomic<bool> bypassed{false};
    std::atomic<bool> panicPending{false};
    std::unique_ptr<std::atomic<float>[]> pendingParameterValues;
    std::unique_ptr<std::atomic<bool>[]> pendingParameterDirty;
    std::atomic<std::size_t> pendingParameterCapacity { 0 };
};

}  // namespace riffra
