#pragma once

#include <memory>

#include "../PluginRack.h"
#include "InstrumentRuntime.h"

namespace riffra {

/// Adapts the existing VST3 rack to the common instrument runtime contract.
class Vst3InstrumentRuntime final : public InstrumentRuntime {
public:
    [[nodiscard]] static std::unique_ptr<Vst3InstrumentRuntime> create(
        const juce::String& path, double sampleRate, int blockSize, const juce::var& persistedState,
        juce::String& error);

    /// Installs an already prepared rack for native integration tests.
    [[nodiscard]] static std::unique_ptr<Vst3InstrumentRuntime> fromRack(
        std::unique_ptr<PluginRack> rack) noexcept;

    ~Vst3InstrumentRuntime() override = default;

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
    [[nodiscard]] PluginRack* vst3Rack() noexcept override;
    [[nodiscard]] const PluginRack* vst3Rack() const noexcept override;

private:
    explicit Vst3InstrumentRuntime(std::unique_ptr<PluginRack> rack) noexcept;

    std::unique_ptr<PluginRack> rack;
};

}  // namespace riffra
