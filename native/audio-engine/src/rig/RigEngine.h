#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>

#include "contract/ExecutionGraph.h"
#include "plugins/PluginChain.h"

namespace riffra {

/// Plays one rig outside the Project: the selected physical input runs
/// through an ordered VST3 effect chain at the rig's gain.
///
/// Plugin construction and state application happen on the VST lifecycle
/// thread. The audio thread only reads the published, prepared instance.
class RigEngine final {
public:
    RigEngine() = default;
    ~RigEngine() = default;

    RigEngine(const RigEngine&) = delete;
    RigEngine& operator=(const RigEngine&) = delete;

    // Lifecycle thread only.
    /// Whether a projection keeps the prepared chain: the same rig, the same
    /// effects in the same order at the same audio format. An open editor and
    /// its prepared instances survive such a projection.
    [[nodiscard]] bool keepsPreparedChain(const RigProjectionSpec& projection, double sampleRate,
                                          int blockSize) const;
    /// Publishes the playing projection. A projection that keeps the prepared
    /// chain applies only bypass: plugin state changes through the device's
    /// editor, which edits the prepared instance directly.
    [[nodiscard]] bool install(const RigProjectionSpec& projection, double sampleRate,
                               int blockSize, juce::String& error);
    /// Fades out and destroys the published rig on the calling thread.
    void uninstall() noexcept;

    // Any control thread.
    /// The prepared rack of one device, kept alive while the handle exists.
    [[nodiscard]] std::shared_ptr<PluginRack> findRack(const juce::String& deviceId);
    [[nodiscard]] juce::String rigId();
    [[nodiscard]] std::uint64_t revision();

    // Audio thread only.
    /// Whether the rig is monitoring its input.
    [[nodiscard]] bool monitorsInput() noexcept;
    /// Physical input channel the playing rig monitors.
    [[nodiscard]] std::uint32_t inputChannel() noexcept;
    /// Adds the rig output to the mix.
    void mix(const float* const* inputChannels, int inputChannelCount, float* const* outputChannels,
             int outputChannelCount, int numSamples, double sampleRate) noexcept;

private:
    struct Installed final {
        RigProjectionSpec projection;
        /// Prepared chain shared with the next projection that keeps it.
        std::shared_ptr<PluginChain> chain;
        juce::AudioBuffer<float> renderBuffer;
        double sampleRate = 0.0;
        int blockSize = 0;
        std::atomic<bool> stopping{false};
        std::atomic<bool> faded{false};
    };

    /// Counts realtime users of the published instance so replacement and
    /// teardown can wait until none of them still reads it.
    class Use final {
    public:
        explicit Use(RigEngine& owner) noexcept;
        ~Use();
        [[nodiscard]] Installed* get() const noexcept { return current; }

    private:
        RigEngine& engine;
        Installed* current;
    };

    std::unique_ptr<Installed> owned;
    std::atomic<Installed*> active{nullptr};
    std::atomic<int> users{0};
};

}  // namespace riffra
