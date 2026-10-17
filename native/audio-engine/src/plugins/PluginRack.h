#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <optional>
#include <span>
#include <vector>

#include "concurrency/BoundedMpmcQueue.h"
#include "contract/ExecutionGraph.h"
#include "timeline/AutomationRuntime.h"

namespace riffra {

class PluginEditorHost;
class PluginRackTestPeer;

/// Processing context a hosted plugin is prepared for (VST3 process mode).
enum class PluginProcessingMode {
    /// Blocks are processed inside a realtime audio callback.
    realtime,
    /// Blocks are processed ahead of realtime, so the plugin may take the time it needs.
    offline,
};

struct PluginLoadError final {
    juce::String scope;
    juce::String message;
};

/// Auxiliary input support of one plugin rack, probed from the plugin bus structure.
struct PluginSidechainCapability final {
    /// An auxiliary input bus is exposed after the main input bus.
    bool available = false;
    /// The auxiliary input bus accepts a mono layout.
    bool mono = false;
    /// The auxiliary input bus accepts a stereo layout.
    bool stereo = false;
    /// Channels of the currently enabled auxiliary layout (0, 1, or 2).
    int channels = 0;
    /// Why the auxiliary input cannot be used, when it cannot be.
    std::optional<juce::String> reason;
};

/// Prepared configuration and runtime counters of one plugin rack.
struct PluginRackStatus final {
    bool loaded = false;
    juce::String path;
    juce::String name;
    bool bypassed = false;
    double sampleRate = 0.0;
    int blockSize = 0;
    int inputChannels = 0;
    int outputChannels = 0;
    std::uint64_t bypassedBlocks = 0;
    std::uint64_t processedBlocks = 0;
    std::uint64_t transitionBlocks = 0;
    std::uint64_t droppedMidiEvents = 0;
    std::uint64_t loadCount = 0;
    std::uint64_t destroyCount = 0;
    PluginSidechainCapability sidechain;
};

/// One normalized discrete value and its plugin-provided text.
struct PluginParameterChoice final {
    float value = 0.0f;
    juce::String displayValue;
};

/// One host-visible plugin parameter.
struct PluginParameterInfo final {
    int index = 0;
    std::optional<juce::String> parameterId;
    juce::String name;
    float value = 0.0f;
    float defaultValue = 0.0f;
    bool automatable = false;
    juce::String displayValue;
    juce::String label;
    bool discrete = false;
    std::uint32_t stepCount = 0;
    std::vector<PluginParameterChoice> choices;
};

/// Programs exposed by a plugin, or the reason they could not be enumerated.
struct PluginProgramStatus final {
    /// Selected program, or -1 when the plugin reports none.
    int currentIndex = -1;
    /// Program names in index order.
    std::vector<juce::String> names;
    std::optional<juce::String> error;
};

/// Plugin-reported state of one parameter, read back after an application.
struct PluginParameterReadback final {
    float value = 0.0f;
    juce::String displayValue;
    juce::String label;
    bool discrete = false;
    std::uint32_t stepCount = 0;
};

class PluginRack;

/// Graph-owned binding; plugin identity and parameter references are resolved before publication.
struct PluginParameterAutomation final {
    PluginRack* rack = nullptr;
    juce::AudioProcessorParameter* parameter = nullptr;
    AutomationRuntime lane;
    float staticValue = 0.0f;
    std::size_t parameterIndex = 0;
    std::int64_t inputLatency = 0;
};

/// Why a projected automation lane could not be bound to this Rack.
enum class AutomationBindingFailure { laneUnavailable, invalidParameter, unavailableStorage };

class PluginRack final {
public:
    PluginRack();
    ~PluginRack();

    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize, PluginProcessingMode mode,
                                                      bool sidechainInput);
    void release() noexcept;
    /// Reserves callback MIDI storage for prepared timeline events.
    [[nodiscard]] bool prepareTimelineMidiCapacity(std::size_t eventCapacity,
                                                   juce::String& error) noexcept;
    void reset() noexcept;
    /// Audio owner restores static parameter values without serializing plugin state.
    void restoreAutomation() noexcept;
    void beginAutomationRead() noexcept { automationActive.store(true, std::memory_order_release); }
    [[nodiscard]] bool isAutomationActive() const noexcept {
        return automationActive.load(std::memory_order_acquire);
    }

    void setBypassed(bool shouldBypass) noexcept;
    bool setParameter(int index, float value, juce::String& error) noexcept;
    bool setProgram(int index, juce::String& error);
    bool applyPersistedState(const PluginStateSpec& state, juce::String& error) noexcept;
    [[nodiscard]] std::optional<PluginStateSpec> persistedState(juce::String& error) const;
    void process(const float* const* inputChannelData, int numInputChannels,
                 const float* const* auxInputChannelData, int numAuxInputChannels,
                 float* const* outputChannelData, int numOutputChannels, int numSamples,
                 const juce::MidiBuffer* timelineMidi = nullptr,
                 std::span<PluginParameterAutomation> automation = {},
                 std::int64_t timelineSample = 0, bool playing = false) noexcept;
    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isLoaded() const noexcept;
    [[nodiscard]] bool isInstrument() const noexcept;
    [[nodiscard]] int latencySamples() const noexcept;
    [[nodiscard]] int tailSamples() const noexcept;
    [[nodiscard]] std::uint64_t droppedMidiEvents() const noexcept;
    [[nodiscard]] PluginRackStatus status() const;
    [[nodiscard]] std::vector<PluginParameterInfo> parameters() const;
    /// Plugin-reported normalized value, display text, and quantization of one
    /// parameter, read back to verify an application.
    [[nodiscard]] std::optional<PluginParameterReadback> parameterReadback(
        int index, juce::String& error) const;
    /// Compares requested display text with the plugin-rendered text, allowing
    /// unit continuations and display precision to differ.
    [[nodiscard]] static bool equivalentDisplayText(const juce::String& input,
                                                    const juce::String& output,
                                                    const juce::String& label);
    /// Resolves a stable parameter ID and binds its lane to the projected static state.
    [[nodiscard]] std::optional<PluginParameterAutomation> bindAutomation(
        const juce::String& parameterId, AutomationRuntime lane, const PluginStateSpec& state,
        bool stepped, AutomationBindingFailure& failure, juce::String& error);
    /// Sets or clears a transient value consumed by the audio thread during automation read.
    [[nodiscard]] bool setAutomationPreview(const juce::String& parameterId,
                                            std::uint64_t previewToken, std::optional<float> value,
                                            juce::String& error);
    [[nodiscard]] PluginProgramStatus programStatus() const;
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
    friend class TimelineEngine;

    static constexpr std::size_t kMaximumPanicMidiEvents = 16 * 3;
    static constexpr std::size_t kMidiEventOverhead = sizeof(std::int32_t) + sizeof(std::uint16_t);

    struct ParameterQueue final {
        explicit ParameterQueue(std::size_t parameterCount) : capacity(parameterCount) {}

        struct Restore final {
            juce::AudioProcessorParameter* parameter = nullptr;
            float value = 0.0f;
        };
        const std::size_t capacity;
        std::unique_ptr<Restore[]> automationRestore;
        std::unique_ptr<std::atomic<float>[]> values;
        std::unique_ptr<std::atomic<bool>[]> dirty;
        std::unique_ptr<AutomationPreviewValue[]> automationPreview;
    };

    void updateParameterCount(juce::AudioProcessor& processor);
    [[nodiscard]] juce::AudioProcessorEditor* createEditor(juce::String& error);
    [[nodiscard]] juce::String currentPluginName() const;
    /// Auxiliary input wiring resolved from the buses a processor was prepared with.
    struct ProcessorLayout final {
        PluginSidechainCapability sidechain;
        std::vector<int> auxInputOffsets;
        int processChannels = 0;
    };
    [[nodiscard]] static std::optional<PluginLoadError> configureProcessor(
        juce::AudioProcessor& processor, double sampleRate, int blockSize,
        PluginProcessingMode mode, bool sidechainInput, ProcessorLayout& layout);
    void adoptProcessorLayout(const ProcessorLayout& layout, int blockSize) noexcept;
    bool applyStateData(const juce::String& base64, juce::String& error) noexcept;
    void applyQueuedParameterChanges() noexcept;
    void applyAutomation(std::span<PluginParameterAutomation> bindings, std::int64_t timelineSample,
                         bool playing) noexcept;
    bool processControlBlock() noexcept;
    juce::AudioBuffer<float> controlBuffer;
    juce::MidiBuffer controlMidi;
    std::atomic<std::uint64_t> requestedControlSequence{0};
    std::atomic<std::uint64_t> completedControlSequence{0};
    std::atomic<std::uint64_t> cancelledControlSequence{0};
    void queueParameterValue(int index, float value) noexcept;
    [[nodiscard]] bool canQueueParameterValue(int index, juce::String& error) const noexcept;
    bool synchronizeParameterController(juce::String& error);
    std::optional<float> parameterValueForText(int index, const juce::String& text,
                                               juce::String& error) const;
    [[nodiscard]] std::optional<juce::String> parameterDisplayValue(int index, float value,
                                                                    juce::String& error) const;
    bool allocateParameterQueue(std::size_t count, juce::String& error) noexcept;

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
    std::unique_ptr<ParameterQueue> parameterQueue;
    juce::AudioBuffer<float> auxInputBuffer;
    std::vector<float*> processPointers;
    std::vector<int> auxInputOffsets;
    int processChannelCount = 0;
    std::atomic<int> preparedLatencySamples{0};
    int preparedTailSamples = 0;
    PendingMidi pendingMidi;
    juce::MidiBuffer processMidi;
    mutable juce::SpinLock pluginLock;
    mutable juce::CriticalSection statusLock;
    std::atomic<std::size_t> cachedParameterCount{0};
    juce::String pluginPath;
    juce::String pluginName;
    std::atomic<double> preparedSampleRate{0.0};
    std::atomic<int> preparedBlockSize{0};
    std::atomic<int> pluginInputChannels{0};
    std::atomic<int> pluginOutputChannels{0};
    std::atomic<int> pluginAuxInputChannels{0};
    std::atomic<bool> pluginIsInstrument{false};
    std::atomic<bool> pluginSidechainAvailable{false};
    std::atomic<bool> pluginSidechainMono{false};
    std::atomic<bool> pluginSidechainStereo{false};
    std::optional<juce::String> sidechainUnavailableReason;
    std::atomic<bool> hostEditInProgress{false};
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
    std::atomic<bool> parameterControllerSyncPending{false};
    std::atomic<bool> automationActive{false};
    const PluginParameterAutomation* activeBindings = nullptr;
    std::size_t activeBindingCount = 0;
    mutable std::optional<PluginStateSpec> staticState;
    mutable std::atomic<bool> staticStateDirty{true};
};

}  // namespace riffra
