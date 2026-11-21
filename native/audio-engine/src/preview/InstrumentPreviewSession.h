#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

#include "contract/InstrumentPreview.h"
#include "instruments/InstrumentRuntime.h"

namespace riffra::instrument_preview {

inline double barPositionForFrame(const std::uint64_t frame, const double sampleRate,
                                  const double tempoBpm, const std::uint8_t numerator,
                                  const std::uint8_t denominator) noexcept {
    if (!std::isfinite(sampleRate) || sampleRate <= 0.0 || !isValidTempo(tempoBpm) ||
        !isValidNumerator(numerator) || !isValidDenominator(denominator))
        return 0.0;
    const auto beatPosition = static_cast<double>(frame) / sampleRate * tempoBpm / 60.0;
    const auto beatsPerBar =
        static_cast<double>(numerator) * 4.0 / static_cast<double>(denominator);
    return beatPosition / beatsPerBar;
}

}  // namespace riffra::instrument_preview

namespace riffra {

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
    [[nodiscard]] bool isFinished() const noexcept {
        return !active.load(std::memory_order_acquire);
    }
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
    std::atomic<bool> active{true};
};

}  // namespace riffra
