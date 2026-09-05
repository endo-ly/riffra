#pragma once

#include <JuceHeader.h>

#include <cstdint>

namespace riffra {

class PluginRack;

struct InstrumentProcessContext final {
    std::uint64_t absoluteFrame = 0;
    double tempoBpm = 120.0;
    double beatPosition = 0.0;
    double barPosition = 0.0;
    std::uint16_t timeSignatureNumerator = 4;
    std::uint16_t timeSignatureDenominator = 4;
    bool playing = false;
};

/// Runtime interface shared by VST3 and Riffra built-in instruments.
class InstrumentRuntime {
public:
    virtual ~InstrumentRuntime() = default;

    [[nodiscard]] virtual bool isLoaded() const noexcept = 0;
    virtual void process(float* const* outputChannels, int outputChannelCount, int numSamples,
                         const juce::MidiBuffer* midi,
                         const InstrumentProcessContext& context) noexcept = 0;
    [[nodiscard]] virtual bool enqueueMidi(const juce::MidiMessage& message) noexcept = 0;
    virtual void allNotesOff() noexcept = 0;
    virtual void resetForTransportDiscontinuity() noexcept = 0;
    [[nodiscard]] virtual int latencySamples() const noexcept = 0;
    [[nodiscard]] virtual int tailSamples() const noexcept = 0;
    virtual void setBypassed(bool shouldBypass) noexcept = 0;
    [[nodiscard]] virtual std::uint32_t faultCode() const noexcept { return 0; }
    [[nodiscard]] virtual std::uint64_t droppedMidiEvents() const noexcept { return 0; }

    [[nodiscard]] virtual PluginRack* vst3Rack() noexcept { return nullptr; }
    [[nodiscard]] virtual const PluginRack* vst3Rack() const noexcept { return nullptr; }
};

}  // namespace riffra
