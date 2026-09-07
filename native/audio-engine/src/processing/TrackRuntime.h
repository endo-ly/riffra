#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>
#include <utility>
#include <vector>

#include "AutomationRuntime.h"
#include "MidiScheduler.h"
#include "PluginChain.h"
#include "RecordingCaptureRuntime.h"
#include "instrument/InstrumentRuntime.h"

namespace riffra {

/// The stage at which a prepared audio take enters the track graph.
enum class ProcessingStage { PreEffects, PostEffects };

/// Owns one Track's realtime DSP state and plugin lifecycle.
///
/// TimelineEngine owns graph publication and processing order. This type owns
/// the state that must move with one Track through preparation, playback, live
/// MIDI, latency compensation, automation, and recording capture.
class TrackRuntime final {
    /// Owns plugin instances that can move between prepared graph generations
    /// when their topology and persisted state are unchanged.
    class TrackDeviceRuntime final {
    public:
        TrackDeviceRuntime() = default;
        ~TrackDeviceRuntime() = default;

        TrackDeviceRuntime(const TrackDeviceRuntime&) = delete;
        TrackDeviceRuntime& operator=(const TrackDeviceRuntime&) = delete;
        TrackDeviceRuntime(TrackDeviceRuntime&&) noexcept = default;
        TrackDeviceRuntime& operator=(TrackDeviceRuntime&&) noexcept = default;

        std::unique_ptr<InstrumentRuntime> instrument;
        PluginChain effects;
        PluginChain recordingEffects;
    };

public:
    TrackRuntime() = default;
    ~TrackRuntime() = default;

    TrackRuntime(const TrackRuntime&) = delete;
    TrackRuntime& operator=(const TrackRuntime&) = delete;
    TrackRuntime(TrackRuntime&&) noexcept = delete;
    TrackRuntime& operator=(TrackRuntime&&) noexcept = delete;

    [[nodiscard]] InstrumentRuntime* instrument() noexcept { return devices.instrument.get(); }
    [[nodiscard]] const InstrumentRuntime* instrument() const noexcept {
        return devices.instrument.get();
    }
    [[nodiscard]] PluginChain& effects() noexcept { return devices.effects; }
    [[nodiscard]] const PluginChain& effects() const noexcept { return devices.effects; }
    [[nodiscard]] PluginChain& recordingEffects() noexcept { return devices.recordingEffects; }
    [[nodiscard]] const PluginChain& recordingEffects() const noexcept {
        return devices.recordingEffects;
    }

    void setInstrument(std::unique_ptr<InstrumentRuntime> runtime) noexcept {
        devices.instrument = std::move(runtime);
    }

    [[nodiscard]] bool hasLoadedInstrument() const noexcept {
        return devices.instrument != nullptr && devices.instrument->isLoaded();
    }

    /// Reserves the timeline MIDI storage owned by the instrument runtime.
    [[nodiscard]] bool prepareTimelineMidiCapacity(const std::size_t eventCapacity,
                                                   juce::String& error) noexcept {
        return devices.instrument == nullptr ||
               devices.instrument->prepareTimelineMidiCapacity(eventCapacity, error);
    }

    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept {
        if (devices.instrument == nullptr || !devices.instrument->enqueueMidi(message))
            return false;
        updateLiveActivity(message);
        liveMidiActiveState.store(true, std::memory_order_release);
        return true;
    }

    void panic() noexcept {
        if (devices.instrument != nullptr) devices.instrument->allNotesOff();
        devices.effects.allNotesOff();
        devices.recordingEffects.allNotesOff();
        heldNotes.store(0, std::memory_order_release);
        sustain.store(false, std::memory_order_release);
        liveTailRemainingSamples.store(std::max(1, totalPluginTailSamples()),
                                       std::memory_order_release);
        liveMidiActiveState.store(false, std::memory_order_release);
    }

    void resetForTransportDiscontinuity() noexcept {
        if (devices.instrument != nullptr) devices.instrument->resetForTransportDiscontinuity();
        devices.effects.allNotesOff();
        devices.recordingEffects.allNotesOff();
        heldNotes.store(0, std::memory_order_release);
        sustain.store(false, std::memory_order_release);
        liveTailRemainingSamples.store(0, std::memory_order_release);
        liveMidiActiveState.store(false, std::memory_order_release);
    }

    [[nodiscard]] bool liveMidiActive() const noexcept {
        return liveMidiActiveState.load(std::memory_order_acquire) ||
               liveTailRemainingSamples.load(std::memory_order_acquire) > 0;
    }

    void markLiveMidiProcessed(const int sampleCount) noexcept {
        if (heldNotes.load(std::memory_order_acquire) > 0 ||
            sustain.load(std::memory_order_acquire))
            return;
        const auto remaining = liveTailRemainingSamples.load(std::memory_order_acquire);
        const auto next = std::max<std::int64_t>(0, remaining - std::max(0, sampleCount));
        liveTailRemainingSamples.store(next, std::memory_order_release);
        if (next == 0) liveMidiActiveState.store(false, std::memory_order_release);
    }

    [[nodiscard]] int pluginLatencySamples() const noexcept {
        return devices.effects.latencySamples() +
               (devices.instrument != nullptr ? devices.instrument->latencySamples() : 0);
    }

    [[nodiscard]] int totalPluginTailSamples() const noexcept {
        return devices.effects.tailSamples() +
               (devices.instrument != nullptr ? devices.instrument->tailSamples() : 0);
    }

    // Prepared timeline state. The snapshot builder allocates these buffers;
    // realtime code only clears and processes their existing storage.
    std::vector<MidiScheduler::CompiledMidiClip> midiClips;
    juce::AudioBuffer<float> mixBuffer;
    juce::AudioBuffer<float> processedBuffer;
    juce::AudioBuffer<float> postEffectClipBuffer;
    juce::AudioBuffer<float> liveInputBuffer;
    RecordingCaptureTrackState recordingCapture;
    juce::AudioBuffer<float> delayBuffer;
    juce::AudioBuffer<float> postEffectDelayBuffer;
    std::int64_t delayWritePosition = 0;
    std::int64_t postEffectDelayWritePosition = 0;
    std::int64_t compensationDelaySamples = 0;
    std::int64_t postEffectCompensationDelaySamples = 0;
    std::int64_t pluginDelaySamples = 0;
    std::int64_t pluginTailSamples = 0;
    std::size_t midiEventCapacity = 0;
    double outputSampleRate = 0.0;
    int preparedBlockSize = 0;
    float gainDb = 0.0f;
    float pan = 0.0f;
    AutomationRuntime volumeAutomation;
    AutomationRuntime panAutomation;
    bool muted = false;
    bool solo = false;
    bool instrumentTrack = false;
    bool armed = false;
    int audioInputChannel = -1;
    bool monitorInput = false;
    bool baseLowLatencyMonitoring = false;
    // Timeline compensation is applied before live input is added, so this
    // policy does not bypass the Timeline path. It records which Track
    // currently owns explicit low-latency MIDI focus for the live boundary.
    bool lowLatencyMonitoring = false;
    juce::String midiDeviceId;
    int midiChannel = 0;
    juce::MidiBuffer midiBuffer;

private:
    friend class TimelineEngine;

    void replaceDeviceRuntimeFrom(TrackRuntime& source) noexcept {
        devices = std::move(source.devices);
    }

    void updateLiveActivity(const juce::MidiMessage& message) noexcept {
        if (message.isNoteOn()) {
            heldNotes.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        if (message.isNoteOff()) {
            auto held = heldNotes.load(std::memory_order_relaxed);
            while (held > 0 &&
                   !heldNotes.compare_exchange_weak(held, held - 1, std::memory_order_relaxed)) {
            }
            if (held <= 1 && !sustain.load(std::memory_order_relaxed))
                liveTailRemainingSamples.store(std::max(1, totalPluginTailSamples()),
                                               std::memory_order_release);
            return;
        }
        if (message.isController() && message.getControllerNumber() == 64) {
            const auto isDown = message.getControllerValue() >= 64;
            sustain.store(isDown, std::memory_order_release);
            if (!isDown && heldNotes.load(std::memory_order_relaxed) == 0)
                liveTailRemainingSamples.store(std::max(1, totalPluginTailSamples()),
                                               std::memory_order_release);
            return;
        }
        if (message.isAllNotesOff() || message.isAllSoundOff()) {
            heldNotes.store(0, std::memory_order_release);
            sustain.store(false, std::memory_order_release);
            liveTailRemainingSamples.store(std::max(1, totalPluginTailSamples()),
                                           std::memory_order_release);
        }
    }

    TrackDeviceRuntime devices;
    std::atomic<bool> liveMidiActiveState{false};
    std::atomic<int> heldNotes{0};
    std::atomic<bool> sustain{false};
    std::atomic<std::int64_t> liveTailRemainingSamples{0};
};

}  // namespace riffra
