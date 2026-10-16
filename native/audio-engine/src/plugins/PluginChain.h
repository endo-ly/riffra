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
                 std::int64_t timelineSample = 0, bool playing = false) noexcept;
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
    /// True when the prepared alignment delays match, so reused racks keep their sidechain
    /// timing without mutation.
    [[nodiscard]] bool usesSameMainInputDelays(
        const std::vector<std::pair<juce::String, int>>& delays) const noexcept;
    /// Applies the alignment delay in front of each device main input. Succeeds without
    /// mutation when the chain already carries the requested alignment.
    bool prepareMainInputDelays(const std::vector<std::pair<juce::String, int>>& delays,
                                juce::String& error) noexcept;
    /// Auxiliary input destination of a prepared sidechain target device.
    [[nodiscard]] juce::AudioBuffer<float>* sidechainBuffer(const juce::String& deviceId) noexcept;
    /// Alignment delay in samples applied to a device main input.
    [[nodiscard]] int mainInputDelay(const juce::String& deviceId) const noexcept;
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
        int mainInputDelaySamples = 0;
        int mainInputDelayPosition = 0;
        juce::AudioBuffer<float> mainInputDelayBuffer;
    };

    void prepareBuffers(int blockSize) noexcept;
    /// Sizes the device auxiliary input buffer; the rack folds it to its enabled layout.
    void prepareSidechainInput(Device& device, int blockSize) noexcept;
    /// Enabled sidechain connection targets, sorted and deduplicated.
    [[nodiscard]] static std::vector<juce::String> enabledSidechainTargets(
        const std::vector<SidechainSendSpec>& sidechainSends);
    /// Alignment entries with zero delays removed.
    [[nodiscard]] static std::vector<std::pair<juce::String, int>> normalizedMainInputDelays(
        const std::vector<std::pair<juce::String, int>>& delays);
    /// Writes the device input through its alignment ring and returns the delayed channels.
    [[nodiscard]] const float* const* delayDeviceInput(Device& device, const float* const* channels,
                                                       int channelCount, int sampleCount) noexcept;

    std::vector<Device> devices;
    std::vector<juce::String> sidechainTargets;
    std::vector<std::pair<juce::String, int>> mainInputDelays;
    juce::AudioBuffer<float> firstBuffer;
    juce::AudioBuffer<float> secondBuffer;
    juce::AudioBuffer<float> delayedInputBuffer;
};

}  // namespace riffra
