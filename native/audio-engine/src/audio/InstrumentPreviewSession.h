#pragma once

#include <JuceHeader.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

#include "../instruments/InstrumentRuntime.h"

namespace riffra {

struct InstrumentPreviewTimeSignature final {
    std::uint8_t numerator = 4;
    std::uint8_t denominator = 4;
};

struct InstrumentPreviewNote final {
    std::uint64_t tick = 0;
    std::uint64_t durationTicks = 0;
    std::uint8_t note = 0;
    std::uint8_t velocity = 0;
};

struct InstrumentPreviewSpec final {
    double tempoBpm = 120.0;
    std::uint16_t ticksPerBeat = 480;
    InstrumentPreviewTimeSignature timeSignature;
    std::uint64_t lengthTicks = 0;
    std::vector<InstrumentPreviewNote> notes;
};

/// Owns the prepared runtime and fixed MIDI schedule for one built-in preview.
class InstrumentPreviewSession final {
public:
    [[nodiscard]] static std::unique_ptr<InstrumentPreviewSession> create(
        const juce::String& definitionJson, const juce::String& definitionBaseDir,
        InstrumentPreviewSpec spec, double sampleRate, int blockSize, juce::String& error);

    ~InstrumentPreviewSession();

    InstrumentPreviewSession(const InstrumentPreviewSession&) = delete;
    InstrumentPreviewSession& operator=(const InstrumentPreviewSession&) = delete;

    /// Runs one already-prepared block and mixes it into the supplied output.
    void process(float* const* outputChannels, int outputChannelCount, int numSamples,
                 double sampleRate) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isFinished() const noexcept { return !active; }
    [[nodiscard]] bool hasFault() const noexcept { return faultCode != 0; }

private:
    struct ScheduledEvent final {
        std::uint64_t frame = 0;
        bool noteOn = false;
        std::uint8_t note = 0;
        std::uint8_t velocity = 0;
    };

    InstrumentPreviewSession(std::unique_ptr<InstrumentRuntime> runtime, InstrumentPreviewSpec spec,
                             double sampleRate, int blockSize, std::vector<ScheduledEvent> events,
                             std::uint64_t lengthFrames) noexcept;

    static std::uint64_t tickToFrame(std::uint64_t tick, const InstrumentPreviewSpec& spec,
                                     double sampleRate) noexcept;

    std::unique_ptr<InstrumentRuntime> runtime;
    InstrumentPreviewSpec spec;
    std::vector<ScheduledEvent> events;
    juce::MidiBuffer midi;
    juce::AudioBuffer<float> renderBuffer;
    std::uint64_t renderedFrames = 0;
    std::uint64_t lengthFrames = 0;
    std::size_t nextEvent = 0;
    int tailFrames = 0;
    int blockSize = 0;
    std::uint32_t faultCode = 0;
    bool active = true;
};

}  // namespace riffra
