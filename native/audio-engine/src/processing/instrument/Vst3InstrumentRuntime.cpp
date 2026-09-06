#include "Vst3InstrumentRuntime.h"

#include <utility>

namespace riffra {

Vst3InstrumentRuntime::Vst3InstrumentRuntime(std::unique_ptr<PluginRack> rack) noexcept
    : rack(std::move(rack)) {}

std::unique_ptr<Vst3InstrumentRuntime> Vst3InstrumentRuntime::create(
    const juce::String& path, const double sampleRate, const int blockSize,
    const juce::var& persistedState, juce::String& error) {
    auto rack = std::make_unique<PluginRack>();
    if (const auto loadError = rack->load(path, sampleRate, blockSize)) {
        error = loadError->scope + ": " + loadError->message;
        return nullptr;
    }
    if (!rack->isInstrument()) {
        error = "The selected VST3 does not provide an instrument output.";
        return nullptr;
    }
    if (!rack->applyPersistedState(persistedState, error)) return nullptr;
    return std::unique_ptr<Vst3InstrumentRuntime>(new Vst3InstrumentRuntime(std::move(rack)));
}

std::unique_ptr<Vst3InstrumentRuntime> Vst3InstrumentRuntime::fromRack(
    std::unique_ptr<PluginRack> rack) noexcept {
    return rack == nullptr
               ? nullptr
               : std::unique_ptr<Vst3InstrumentRuntime>(new Vst3InstrumentRuntime(std::move(rack)));
}

bool Vst3InstrumentRuntime::isLoaded() const noexcept {
    return rack != nullptr && rack->isLoaded() && rack->isInstrument();
}

void Vst3InstrumentRuntime::process(float* const* outputChannels, const int outputChannelCount,
                                    const int numSamples, const juce::MidiBuffer* const midi,
                                    const InstrumentProcessContext&) noexcept {
    if (rack == nullptr) {
        if (outputChannels != nullptr)
            for (int channel = 0; channel < outputChannelCount; ++channel)
                if (outputChannels[channel] != nullptr)
                    juce::FloatVectorOperations::clear(outputChannels[channel], numSamples);
        return;
    }
    rack->process(nullptr, 0, outputChannels, outputChannelCount, numSamples, midi);
}

bool Vst3InstrumentRuntime::enqueueMidi(const juce::MidiMessage& message) noexcept {
    if (!isLoaded()) return false;
    rack->enqueueMidi(message);
    return true;
}

void Vst3InstrumentRuntime::allNotesOff() noexcept {
    if (rack != nullptr) rack->allNotesOff();
}

void Vst3InstrumentRuntime::resetForTransportDiscontinuity() noexcept { allNotesOff(); }

int Vst3InstrumentRuntime::latencySamples() const noexcept {
    return rack != nullptr ? rack->latencySamples() : 0;
}

int Vst3InstrumentRuntime::tailSamples() const noexcept {
    return rack != nullptr ? rack->tailSamples() : 0;
}

void Vst3InstrumentRuntime::setBypassed(const bool shouldBypass) noexcept {
    if (rack != nullptr) rack->setBypassed(shouldBypass);
}

PluginRack* Vst3InstrumentRuntime::vst3Rack() noexcept { return rack.get(); }

const PluginRack* Vst3InstrumentRuntime::vst3Rack() const noexcept { return rack.get(); }

}  // namespace riffra
