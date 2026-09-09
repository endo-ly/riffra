#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <optional>
#include <vector>

#include "BoundedMpmcQueue.h"

namespace riffra {

class PluginEditorHost;
class PluginRackTestPeer;

struct PluginLoadError final {
    juce::String scope;
    juce::String message;
};

class PluginRack final {
public:
    PluginRack();
    ~PluginRack();

    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize);
    void clear() noexcept;
    void release() noexcept;
    void prepare(double sampleRate, int blockSize) noexcept;
    /// Reserves callback MIDI storage for prepared timeline events.
    [[nodiscard]] bool prepareTimelineMidiCapacity(std::size_t eventCapacity,
                                                   juce::String& error) noexcept;
    void reset() noexcept;
    void setBypassed(bool shouldBypass) noexcept;
    bool setParameter(int index, float value, juce::String& error) noexcept;
    bool setProgram(int index, juce::String& error);
    bool applyPersistedState(const juce::var& state, juce::String& error) noexcept;
    [[nodiscard]] juce::var persistedState(juce::String& error) const;
    void process(const float* const* inputChannelData, int numInputChannels,
                 float* const* outputChannelData, int numOutputChannels, int numSamples,
                 const juce::MidiBuffer* timelineMidi = nullptr) noexcept;
    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isLoaded() const noexcept;
    [[nodiscard]] bool isInstrument() const noexcept;
    [[nodiscard]] int latencySamples() const noexcept;
    [[nodiscard]] int tailSamples() const noexcept;
    [[nodiscard]] std::uint64_t droppedMidiEvents() const noexcept;
    [[nodiscard]] juce::var status() const;
    [[nodiscard]] juce::var parameterStatus() const;
    [[nodiscard]] juce::var programStatus() const;
    [[nodiscard]] bool hasPrograms() const noexcept;
    [[nodiscard]] bool hasEditor() const noexcept;
    [[nodiscard]] std::size_t parameterCount() const noexcept;
    void addProcessorListener(juce::AudioProcessorListener& listener) noexcept;
    void removeProcessorListener(juce::AudioProcessorListener& listener) noexcept;
    /// Queues a live-only editor parameter change for the next audio block.
    void enqueueParameterChange(int index, float value) noexcept;

private:
    friend class PluginEditorHost;
    friend class PluginRackTestPeer;

    static constexpr std::size_t kMaximumPanicMidiEvents = 16 * 3;
    static constexpr std::size_t kMidiEventOverhead = sizeof(std::int32_t) + sizeof(std::uint16_t);

    struct CachedParameter {
        int index = 0;
        juce::String name;
        float value = 0.0f;
        float defaultValue = 0.0f;
        bool automatable = false;
    };

    struct ParameterQueue final {
        explicit ParameterQueue(std::size_t parameterCount) : capacity(parameterCount) {}

        const std::size_t capacity;
        std::unique_ptr<std::atomic<float>[]> values;
        std::unique_ptr<std::atomic<bool>[]> dirty;
    };

    void updateParameterCache(juce::AudioProcessor& processor);
    [[nodiscard]] juce::AudioProcessorEditor* createEditor(juce::String& error);
    [[nodiscard]] juce::String currentPluginName() const;
    [[nodiscard]] static std::optional<PluginLoadError> configureProcessor(
        juce::AudioProcessor& processor, double sampleRate, int blockSize);
    [[nodiscard]] juce::var cachedStatus(bool includeParameters) const;
    bool applyStateData(const juce::String& base64, juce::String& error) noexcept;
    void applyQueuedParameterChanges(juce::AudioProcessor* processor,
                                     ParameterQueue* queue) noexcept;
    bool allocateParameterQueue(std::size_t count, juce::String& error) noexcept;
    void reclaimRetiredPlugins() noexcept;

    class PendingMidi final {
    public:
        // Live MIDI packets use a fixed approximately 64 KiB budget and
        // accept messages up to 256 bytes. Larger packets are overflow.
        static constexpr std::size_t kCapacity = 256;
        static constexpr std::size_t kMaximumMessageBytes = 256;

        void reset();
        [[nodiscard]] bool add(const juce::MidiMessage& message) noexcept;
        void appendTo(juce::MidiBuffer& destination, int sampleCount) noexcept;
        void recordDropped() noexcept;
        [[nodiscard]] std::uint64_t droppedEvents() const noexcept;

    private:
        struct Event final {
            std::array<std::uint8_t, kMaximumMessageBytes> bytes{};
            std::uint16_t size = 0;
        };

        BoundedMpmcQueue<Event, kCapacity> messages;
        std::atomic<std::uint64_t> droppedEventsCount{0};
    };

    juce::AudioPluginFormatManager formatManager;
    std::unique_ptr<juce::AudioProcessor> plugin;
    // The audio thread only observes this immutable processing pointer. Plugin
    // lifecycle work is performed on a candidate and published by swapping
    // this pointer; old instances are reclaimed after the reader count drops.
    std::atomic<juce::AudioProcessor*> activePlugin{nullptr};
    std::vector<std::unique_ptr<juce::AudioProcessor>> retiredPlugins;
    std::atomic<std::uint32_t> activeReaders{0};
    std::atomic<ParameterQueue*> activeParameterQueue{nullptr};
    std::unique_ptr<ParameterQueue> parameterQueue;
    std::vector<std::unique_ptr<ParameterQueue>> retiredParameterQueues;
    PendingMidi pendingMidi;
    juce::MidiBuffer processMidi;
    mutable juce::SpinLock pluginLock;
    mutable juce::CriticalSection statusLock;
    std::vector<CachedParameter> cachedParameters;
    juce::String pluginPath;
    juce::String pluginName;
    std::atomic<double> preparedSampleRate{0.0};
    std::atomic<int> preparedBlockSize{0};
    std::atomic<int> pluginInputChannels{0};
    std::atomic<int> pluginOutputChannels{0};
    std::atomic<int> cachedProgramCount{0};
    std::atomic<bool> cachedHasEditor{false};
    std::atomic<bool> loaded{false};
    std::atomic<std::uint64_t> bypassedBlocks{0};
    std::atomic<std::uint64_t> processedBlocks{0};
    std::atomic<std::uint64_t> transitionBlocks{0};
    std::atomic<std::uint64_t> loadCount{0};
    std::atomic<std::uint64_t> destroyCount{0};
    std::atomic<bool> bypassed{false};
    std::atomic<bool> panicPending{false};
};

}  // namespace riffra
