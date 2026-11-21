#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <memory>

#include "plugins/PluginRack.h"

namespace riffra {

/// Plays one VST3 outside the Project, the way its standalone application
/// would: an instrument takes live MIDI ahead of any Track, an effect
/// processes the selected audio input, and the output is mixed after the
/// Arrangement. Nothing it does is persisted.
class PluginAudition final {
public:
    PluginAudition() = default;
    ~PluginAudition() = default;

    PluginAudition(const PluginAudition&) = delete;
    PluginAudition& operator=(const PluginAudition&) = delete;

    // Lifecycle thread only.
    /// Starts playing `rack`, which must be prepared for `sampleRate` and
    /// blocks of at most `blockSize` frames.
    void install(std::unique_ptr<PluginRack> rack, double sampleRate, int blockSize);
    /// Fades the installed plug-in out, stops every realtime use of it, and
    /// returns it so the caller destroys it on its own thread.
    [[nodiscard]] std::unique_ptr<PluginRack> uninstall() noexcept;
    [[nodiscard]] PluginRack* installed() const noexcept;

    // MIDI callback threads.
    /// Queues live MIDI for an installed instrument; false when there is none.
    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept;
    /// Queues all-notes-off for an installed instrument.
    void panic() noexcept;

    // Audio thread only.
    /// Whether an installed effect is monitoring the audio input.
    [[nodiscard]] bool monitorsInput() noexcept;
    /// Adds the plug-in's output, feeding an effect `input` (may be null).
    void mix(const float* input, float* const* outputChannels, int outputChannelCount,
             int numSamples, double sampleRate) noexcept;

private:
    struct Installed final {
        std::unique_ptr<PluginRack> rack;
        bool instrument = false;
        juce::AudioBuffer<float> renderBuffer;
        double sampleRate = 0.0;
        int blockSize = 0;
        std::atomic<bool> stopping{false};
        std::atomic<bool> faded{false};
    };

    /// Counts the realtime threads currently reading `active`, so uninstall
    /// can wait until none of them still holds the previous plug-in.
    class Use final {
    public:
        explicit Use(PluginAudition& owner) noexcept;
        ~Use();
        [[nodiscard]] Installed* get() const noexcept { return current; }

    private:
        PluginAudition& audition;
        Installed* current;
    };

    std::unique_ptr<Installed> owned;
    std::atomic<Installed*> active{nullptr};
    std::atomic<int> users{0};
};

}  // namespace riffra
