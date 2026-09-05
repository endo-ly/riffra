#pragma once

#include <sonalloy.h>

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <memory>

#include "../BoundedMpmcQueue.h"
#include "InstrumentRuntime.h"

namespace riffra {

/// Runs a compiled Riffra built-in instrument through the pinned C API.
class SonalloyInstrumentRuntime final : public InstrumentRuntime {
public:
    [[nodiscard]] static std::unique_ptr<SonalloyInstrumentRuntime> create(
        const juce::String& definitionJson, const juce::String& definitionBaseDir,
        double sampleRate, int blockSize, juce::String& error);

    ~SonalloyInstrumentRuntime() override = default;

    [[nodiscard]] bool isLoaded() const noexcept override;
    void process(float* const* outputChannels, int outputChannelCount, int numSamples,
                 const juce::MidiBuffer* midi,
                 const InstrumentProcessContext& context) noexcept override;
    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept override;
    void allNotesOff() noexcept override;
    void resetForTransportDiscontinuity() noexcept override;
    [[nodiscard]] int latencySamples() const noexcept override;
    [[nodiscard]] int tailSamples() const noexcept override;
    void setBypassed(bool shouldBypass) noexcept override;

    [[nodiscard]] std::uint32_t faultCode() const noexcept override;
    [[nodiscard]] std::uint64_t droppedMidiEvents() const noexcept override;

private:
    struct CompiledDeleter final {
        void operator()(SonalloyCompiledInstrument* value) const noexcept;
    };
    struct DiagnosticsDeleter final {
        void operator()(SonalloyDiagnostics* value) const noexcept;
    };
    struct RuntimeDeleter final {
        void operator()(SonalloyRuntime* value) const noexcept;
    };
    using CompiledPtr = std::unique_ptr<SonalloyCompiledInstrument, CompiledDeleter>;
    using RuntimePtr = std::unique_ptr<SonalloyRuntime, RuntimeDeleter>;

    struct PendingMidi final {
        static constexpr std::size_t kMaximumMessageBytes = 256;
        std::array<std::uint8_t, kMaximumMessageBytes> bytes{};
        std::uint16_t size = 0;
        std::uint64_t generation = 0;
    };

    struct ActiveNote final {
        bool active = false;
        std::uint8_t channel = 0;
        std::uint8_t noteNumber = 0;
        std::uint64_t noteId = 0;
        std::uint64_t startedOrder = 0;
        std::uint64_t startedBlock = 0;
        std::uint32_t startedSampleOffset = 0;
    };

    static constexpr std::size_t kMaximumEventsPerBlock = 1024;
    static constexpr std::size_t kMaximumActiveNotes = 4096;
    static constexpr std::size_t kMaximumPendingMidi = 256;

    SonalloyInstrumentRuntime(CompiledPtr compiled, RuntimePtr runtime, int blockSize,
                              int latencySamples) noexcept;

    [[nodiscard]] static juce::String diagnosticsSummary(const SonalloyDiagnostics* diagnostics,
                                                         SonalloyResult result);
    [[nodiscard]] bool appendMidiBytes(const std::uint8_t* data, std::size_t size,
                                       std::uint32_t sampleOffset,
                                       std::uint32_t& eventCount) noexcept;
    [[nodiscard]] bool appendPendingMidi(const PendingMidi& pending,
                                         std::uint32_t& eventCount) noexcept;
    [[nodiscard]] bool appendNoteOn(std::uint8_t channel, std::uint8_t noteNumber,
                                    std::uint8_t velocity, std::uint32_t sampleOffset,
                                    std::uint32_t& eventCount) noexcept;
    [[nodiscard]] bool appendNoteOff(std::uint8_t channel, std::uint8_t noteNumber,
                                     std::uint32_t sampleOffset,
                                     std::uint32_t& eventCount) noexcept;
    [[nodiscard]] bool appendEvent(SonalloyEvent event, std::uint32_t& eventCount) noexcept;
    [[nodiscard]] bool resetRuntimeInCallback() noexcept;
    void failBlock(std::uint32_t code, float* const* outputChannels, int outputChannelCount,
                   int numSamples) noexcept;
    void clearOutputs(float* const* outputChannels, int outputChannelCount,
                      int numSamples) noexcept;
    void clearActiveNotes() noexcept;
    [[nodiscard]] ActiveNote* findLatestActiveNote(std::uint8_t channel, std::uint8_t noteNumber,
                                                   std::uint32_t sampleOffset) noexcept;
    [[nodiscard]] ActiveNote* allocateActiveNote() noexcept;
    [[nodiscard]] static int eventPriority(const SonalloyEvent& event) noexcept;
    [[nodiscard]] static juce::String resultName(SonalloyResult result);

    CompiledPtr compiled;
    RuntimePtr runtime;
    BoundedMpmcQueue<PendingMidi, kMaximumPendingMidi> pendingMidi;
    std::array<SonalloyEvent, kMaximumEventsPerBlock> events{};
    std::array<ActiveNote, kMaximumActiveNotes> activeNotes{};
    std::atomic<bool> resetPending{false};
    std::atomic<std::uint64_t> midiGeneration{0};
    std::atomic<bool> bypassed{false};
    std::atomic<std::uint32_t> lastFaultCode{0};
    std::atomic<std::uint64_t> droppedMidi{0};
    std::uint64_t absoluteFrame = 0;
    std::uint64_t nextNoteId = 1;
    std::uint64_t processBlockSerial = 0;
    int blockSize = 0;
    int reportedLatencySamples = 0;
};

}  // namespace riffra
