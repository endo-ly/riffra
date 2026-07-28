#pragma once

#include "PluginRack.h"

#include <memory>
#include <vector>

namespace riffra {

class PluginChainTestPeer;

class PluginChain final {
public:
    bool load(
        const juce::var& devices,
        double sampleRate,
        int blockSize,
        juce::String& error);
    void prepare(double sampleRate, int blockSize) noexcept;
    void reset() noexcept;
    void release() noexcept;
    void clear() noexcept;
    void allNotesOff() noexcept;
    void process(
        const float* const* inputChannels,
        int inputChannelCount,
        float* const* outputChannels,
        int outputChannelCount,
        int sampleCount,
        const juce::MidiBuffer* midi = nullptr) noexcept;
    bool setBypassed(const juce::String& deviceId, bool bypassed) noexcept;
    bool setParameter(
        const juce::String& deviceId,
        int parameterIndex,
        float value,
        juce::String& error) noexcept;
    bool applyState(const juce::var& devices, juce::String& error) noexcept;
    [[nodiscard]] juce::var persistedState(
        const juce::String& deviceId,
        juce::String& error) const;
    [[nodiscard]] int latencySamples() const noexcept;
    [[nodiscard]] int tailSamples() const noexcept;
    [[nodiscard]] int size() const noexcept;
    [[nodiscard]] PluginRack* findDevice(const juce::String& deviceId) noexcept;
    [[nodiscard]] const PluginRack* findDevice(const juce::String& deviceId) const noexcept;

private:
    friend class PluginChainTestPeer;

    struct Device final {
        juce::String id;
        std::unique_ptr<PluginRack> rack;
    };

    std::vector<Device> devices;
    juce::AudioBuffer<float> firstBuffer;
    juce::AudioBuffer<float> secondBuffer;
};

} // namespace riffra
