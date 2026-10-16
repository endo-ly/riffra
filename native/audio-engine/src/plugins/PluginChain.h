#pragma once

#include <memory>
#include <optional>
#include <utility>
#include <vector>

#include "PluginRack.h"
#include "contract/ExecutionGraph.h"

namespace riffra {

class PluginChainTestPeer;
class TimelineEngineTestPeer;

/// Graph-owned main-input delays required to align an effect chain's sidechain inputs.
class PluginChainAlignment final {
public:
    bool prepare(const std::vector<juce::String>& deviceIds,
                 const std::vector<std::pair<juce::String, int>>& delays, int blockSize,
                 juce::String& error);
    void reset() noexcept;
    [[nodiscard]] int delaySamples(const juce::String& deviceId) const noexcept;
    [[nodiscard]] int latencySamples() const noexcept { return totalDelaySamples; }
    [[nodiscard]] const float* const* delayDeviceInput(const juce::String& deviceId,
                                                       const float* const* channels,
                                                       int channelCount, int sampleCount) noexcept;

private:
    struct Delay final {
        juce::String deviceId;
        int samples = 0;
        int position = 0;
        juce::AudioBuffer<float> ring;
    };

    std::vector<Delay> delays;
    juce::AudioBuffer<float> delayedInputBuffer;
    int totalDelaySamples = 0;
};

class PluginChain final {
public:
    bool load(const std::vector<PluginDeviceSpec>& devices,
              const std::vector<SidechainSendSpec>& sidechainSends, double sampleRate,
              int blockSize, PluginProcessingMode mode, juce::String& error,
              const juce::String& runtimeRole);
    void reset() noexcept;
    void release() noexcept;
    void clear() noexcept;
    void allNotesOff() noexcept;
    void process(const float* const* inputChannels, int inputChannelCount,
                 float* const* outputChannels, int outputChannelCount, int sampleCount,
                 const juce::MidiBuffer* midi = nullptr,
                 std::span<PluginParameterAutomation> automation = {},
                 std::int64_t timelineSample = 0, bool playing = false,
                 PluginChainAlignment* inputAlignment = nullptr) noexcept;
    bool setBypassed(const juce::String& deviceId, bool bypassed) noexcept;
    bool setParameter(const juce::String& deviceId, int parameterIndex, float value,
                      juce::String& error) noexcept;
    bool applyState(const std::vector<PluginDeviceSpec>& devices, juce::String& error) noexcept;
    [[nodiscard]] std::optional<PluginStateSpec> persistedState(const juce::String& deviceId,
                                                                juce::String& error) const;
    [[nodiscard]] int latencySamples() const noexcept;
    [[nodiscard]] int tailSamples() const noexcept;
    [[nodiscard]] int size() const noexcept;
    /// True when the enabled sidechain connections resolve to the same device targets as the
    /// prepared chain, so its auxiliary inputs stay valid.
    [[nodiscard]] bool usesSameSidechainTargets(
        const std::vector<SidechainSendSpec>& sidechainSends) const noexcept;
    /// Auxiliary input destination of a prepared sidechain target device.
    [[nodiscard]] juce::AudioBuffer<float>* sidechainBuffer(const juce::String& deviceId) noexcept;
    [[nodiscard]] const juce::AudioBuffer<float>* sidechainBuffer(
        const juce::String& deviceId) const noexcept;
    void clearSidechainBuffers() noexcept;
    void resetSidechainState() noexcept;
    [[nodiscard]] PluginRack* findDevice(const juce::String& deviceId) noexcept;
    [[nodiscard]] const PluginRack* findDevice(const juce::String& deviceId) const noexcept;

private:
    friend class PluginChainTestPeer;
    friend class TimelineEngineTestPeer;

    struct Device final {
        juce::String id;
        std::unique_ptr<PluginRack> rack;
        juce::AudioBuffer<float> sidechainBuffer;
        bool sidechainCapable = false;
    };

    void prepareBuffers(int blockSize) noexcept;
    /// Sizes the device auxiliary input buffer; the rack folds it to its enabled layout.
    void prepareSidechainInput(Device& device, int blockSize) noexcept;
    /// Enabled sidechain targets belonging to the supplied device chain.
    [[nodiscard]] static std::vector<juce::String> enabledSidechainTargets(
        const std::vector<juce::String>& deviceIds,
        const std::vector<SidechainSendSpec>& sidechainSends);
    std::vector<Device> devices;
    std::vector<juce::String> sidechainTargets;
    juce::AudioBuffer<float> firstBuffer;
    juce::AudioBuffer<float> secondBuffer;
};

}  // namespace riffra
