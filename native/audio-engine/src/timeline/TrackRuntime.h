#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <memory>
#include <utility>
#include <vector>

#include "AutomationRuntime.h"
#include "MidiScheduler.h"
#include "instruments/InstrumentRuntime.h"
#include "plugins/PluginChain.h"
#include "recording/RecordingCaptureRuntime.h"

namespace riffra {

/// The stage at which a prepared audio take enters the track graph.
enum class ProcessingStage { PreEffects, PostEffects };

struct TrackMeterSnapshot final {
    float peakLeft = 0.0f;
    float peakRight = 0.0f;
    float rmsLeft = 0.0f;
    float rmsRight = 0.0f;
};

/// Holds the largest block meter values until the telemetry thread consumes them.
/// The audio thread only performs atomic peak-holds; no audio buffer is shared
/// with the control or telemetry threads.
class TrackMeterAccumulator final {
public:
    void recordBlock(const float peakLeft, const float peakRight, const float rmsLeft,
                     const float rmsRight) noexcept {
        holdPeak(peakLeftValue, peakLeft);
        holdPeak(peakRightValue, peakRight);
        holdPeak(rmsLeftValue, rmsLeft);
        holdPeak(rmsRightValue, rmsRight);
    }

    [[nodiscard]] TrackMeterSnapshot consume() noexcept {
        return {
            peakLeftValue.exchange(0.0f, std::memory_order_acq_rel),
            peakRightValue.exchange(0.0f, std::memory_order_acq_rel),
            rmsLeftValue.exchange(0.0f, std::memory_order_acq_rel),
            rmsRightValue.exchange(0.0f, std::memory_order_acq_rel),
        };
    }

    void reset() noexcept {
        peakLeftValue.store(0.0f, std::memory_order_release);
        peakRightValue.store(0.0f, std::memory_order_release);
        rmsLeftValue.store(0.0f, std::memory_order_release);
        rmsRightValue.store(0.0f, std::memory_order_release);
    }

private:
    static void holdPeak(std::atomic<float>& peak, const float value) noexcept {
        if (!std::isfinite(value) || value <= 0.0f) return;
        auto current = peak.load(std::memory_order_relaxed);
        while (value > current &&
               !peak.compare_exchange_weak(current, value, std::memory_order_release,
                                           std::memory_order_relaxed)) {
        }
    }

    std::atomic<float> peakLeftValue{0.0f};
    std::atomic<float> peakRightValue{0.0f};
    std::atomic<float> rmsLeftValue{0.0f};
    std::atomic<float> rmsRightValue{0.0f};
};

/// Owns one Track's realtime DSP state and plugin lifecycle.
///
/// TimelineEngine owns graph publication and processing order. This type owns
/// the state that must move with one Track through preparation, playback, live
/// MIDI, latency compensation, automation, and recording capture.
class TrackRuntime final {
    /// Owns the instrument instance and its timeline MIDI capacity. Consecutive
    /// graphs share the block while the instrument topology and persisted state
    /// stay unchanged. Only the active graph processes it.
    struct InstrumentBlock final {
        std::unique_ptr<InstrumentRuntime> runtime;
        std::size_t timelineMidiCapacity = 0;
    };

public:
    TrackRuntime() = default;
    ~TrackRuntime() = default;

    TrackRuntime(const TrackRuntime&) = delete;
    TrackRuntime& operator=(const TrackRuntime&) = delete;
    TrackRuntime(TrackRuntime&&) noexcept = delete;
    TrackRuntime& operator=(TrackRuntime&&) noexcept = delete;

    [[nodiscard]] InstrumentRuntime* instrument() noexcept {
        return instrumentBlock->runtime.get();
    }
    [[nodiscard]] const InstrumentRuntime* instrument() const noexcept {
        return instrumentBlock->runtime.get();
    }
    [[nodiscard]] PluginChain& effects() noexcept { return *effectChain; }
    [[nodiscard]] const PluginChain& effects() const noexcept { return *effectChain; }

    void setInstrument(std::unique_ptr<InstrumentRuntime> runtime) noexcept {
        instrumentBlock->runtime = std::move(runtime);
    }

    [[nodiscard]] bool hasLoadedInstrument() const noexcept {
        return instrumentBlock->runtime != nullptr && instrumentBlock->runtime->isLoaded();
    }

    /// Shares the committed graph's Track instrument block with this runtime.
    /// Control side, while the new graph is being prepared.
    void shareInstrumentWith(const TrackRuntime& committed) noexcept {
        instrumentBlock = committed.instrumentBlock;
    }

    /// Shares the committed graph's Track effect chain with this runtime so
    /// reused plugin instances keep their state. Control side, while the new
    /// graph is being prepared.
    void shareEffectsWith(const TrackRuntime& committed) noexcept {
        effectChain = committed.effectChain;
    }

    /// Effect chain handle the snapshot builder may replace while a graph is
    /// being prepared.
    [[nodiscard]] std::shared_ptr<PluginChain>& effectChainHandle() noexcept { return effectChain; }

    /// Timeline MIDI events per block that the instrument instance can accept.
    [[nodiscard]] std::size_t timelineMidiCapacity() const noexcept {
        return instrumentBlock->timelineMidiCapacity;
    }

    void setLowLatencyMonitoring(const bool enabled) noexcept {
        lowLatencyMonitoringState.store(enabled, std::memory_order_release);
    }

    [[nodiscard]] bool lowLatencyMonitoring() const noexcept {
        return lowLatencyMonitoringState.load(std::memory_order_acquire);
    }

    void setLowLatencyPdcBypassActive(const bool active) noexcept {
        lowLatencyPdcBypassActiveState.store(active, std::memory_order_release);
    }

    [[nodiscard]] bool lowLatencyPdcBypassActive() const noexcept {
        return lowLatencyPdcBypassActiveState.load(std::memory_order_acquire);
    }

    void prepareTimelineInputDelay(const int samples) {
        timelineInputDelaySamples = std::max(0, samples);
        timelineInputDelayBuffer.setSize(2, timelineInputDelaySamples + 1, false, true, false);
        resetTimelineInputDelay();
    }

    void processTimelineInputDelay(juce::AudioBuffer<float>& input, const int sampleCount,
                                   const bool active) noexcept {
        if (active != timelineInputDelayActive) {
            timelineInputDelayBuffer.clear();
            timelineInputDelayWritePosition = 0;
            timelineInputDelayActive = active;
        }
        if (!active || timelineInputDelaySamples == 0) return;

        const auto bufferSize = timelineInputDelayBuffer.getNumSamples();
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto read = (timelineInputDelayWritePosition + 1) % bufferSize;
            for (int channel = 0; channel < 2; ++channel) {
                const auto value = input.getSample(channel, sample);
                timelineInputDelayBuffer.setSample(channel, timelineInputDelayWritePosition, value);
                input.setSample(channel, sample, timelineInputDelayBuffer.getSample(channel, read));
            }
            timelineInputDelayWritePosition = read;
        }
    }

    void resetTimelineInputDelay() noexcept {
        timelineInputDelayBuffer.clear();
        timelineInputDelayWritePosition = 0;
        timelineInputDelayActive = false;
    }

    /// Reserves the timeline MIDI storage owned by the instrument runtime.
    [[nodiscard]] bool prepareTimelineMidiCapacity(const std::size_t eventCapacity,
                                                   juce::String& error) noexcept {
        if (instrumentBlock->runtime != nullptr &&
            !instrumentBlock->runtime->prepareTimelineMidiCapacity(eventCapacity, error))
            return false;
        instrumentBlock->timelineMidiCapacity = eventCapacity;
        return true;
    }

    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept {
        if (instrumentBlock->runtime == nullptr || !instrumentBlock->runtime->enqueueMidi(message))
            return false;
        updateLiveActivity(message);
        liveMidiActiveState.store(true, std::memory_order_release);
        return true;
    }

    void panic() noexcept {
        if (instrumentBlock->runtime != nullptr) instrumentBlock->runtime->allNotesOff();
        effectChain->allNotesOff();
        heldNotes.store(0, std::memory_order_release);
        sustain.store(false, std::memory_order_release);
        liveTailRemainingSamples.store(std::max<std::int64_t>(1, pluginTailSamples),
                                       std::memory_order_release);
        liveMidiActiveState.store(false, std::memory_order_release);
    }

    void resetForTransportDiscontinuity() noexcept { requestTransportDiscontinuity(); }

    void requestTransportDiscontinuity() noexcept {
        if (instrumentBlock->runtime != nullptr)
            instrumentBlock->runtime->resetForTransportDiscontinuity();
        effectChain->allNotesOff();
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
        return effectChain->latencySamples() + (instrumentBlock->runtime != nullptr
                                                    ? instrumentBlock->runtime->latencySamples()
                                                    : 0);
    }

    [[nodiscard]] int totalPluginTailSamples() const noexcept {
        return effectChain->tailSamples() +
               (instrumentBlock->runtime != nullptr ? instrumentBlock->runtime->tailSamples() : 0);
    }

    // Prepared timeline state. The snapshot builder allocates these buffers;
    // realtime code only clears and processes their existing storage.
    std::vector<MidiScheduler::CompiledMidiClip> midiClips;
    std::int64_t timelinePdcDelaySamples = 0;
    juce::AudioBuffer<float> mixBuffer;
    juce::AudioBuffer<float> processedBuffer;
    juce::AudioBuffer<float> trackOutputBuffer;
    juce::AudioBuffer<float> routedOutputBuffer;
    juce::AudioBuffer<float> preFaderOutputBuffer;
    bool mainOutputAudible = true;
    std::int64_t routeLatencySamples = 0;
    TrackRuntime* externalAudioSource = nullptr;
    bool unityCenterStereo = false;
    std::uint64_t windowProcessingTotalUs = 0;
    std::uint64_t windowProcessingMaximumUs = 0;
    std::uint32_t windowProcessingCount = 0;
    std::atomic<std::uint32_t> windowAverageUs{0};
    std::atomic<std::uint32_t> windowMaximumUs{0};
    juce::AudioBuffer<float> postEffectClipBuffer;
    juce::AudioBuffer<float> liveInputBuffer;
    RecordingCaptureTrackState recordingCapture;
    juce::AudioBuffer<float> postEffectDelayBuffer;
    std::int64_t postEffectDelayWritePosition = 0;
    std::int64_t postEffectCompensationDelaySamples = 0;
    bool postEffectLowLatencyBypassActive = false;
    std::int64_t pluginDelaySamples = 0;
    std::int64_t pluginTailSamples = 0;
    std::size_t midiEventCapacity = 0;
    /// Process-wide key of the Track; see TrackKeyRegistry.
    std::uint32_t key = 0;
    double outputSampleRate = 0.0;
    int preparedBlockSize = 0;
    std::atomic<float> gainDb{0.0f};
    std::atomic<float> pan{0.0f};
    AutomationPreviewValue gainPreview;
    AutomationPreviewValue panPreview;
    TrackMeterAccumulator meter;
    std::vector<PluginParameterAutomation> deviceAutomation;
    AutomationRuntime volumeAutomation;
    AutomationRuntime panAutomation;
    bool muted = false;
    bool solo = false;
    bool instrumentTrack = false;
    bool armed = false;
    int audioInputChannel = -1;
    bool monitorInput = false;
    juce::String midiDeviceId;
    /// MidiSourceRegistry index of `midiDeviceId`.
    std::uint16_t midiSourceIndex = 0;
    int midiChannel = 0;
    juce::MidiBuffer midiBuffer;
    std::vector<SonalloyEvent> instrumentEvents;
    std::vector<InstrumentEventOrder> instrumentEventOrdering;
    std::size_t instrumentEventCount = 0;

private:
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
                liveTailRemainingSamples.store(std::max<std::int64_t>(1, pluginTailSamples),
                                               std::memory_order_release);
            return;
        }
        if (message.isController() && message.getControllerNumber() == 64) {
            const auto isDown = message.getControllerValue() >= 64;
            sustain.store(isDown, std::memory_order_release);
            if (!isDown && heldNotes.load(std::memory_order_relaxed) == 0)
                liveTailRemainingSamples.store(std::max<std::int64_t>(1, pluginTailSamples),
                                               std::memory_order_release);
            return;
        }
        if (message.isAllNotesOff() || message.isAllSoundOff()) {
            heldNotes.store(0, std::memory_order_release);
            sustain.store(false, std::memory_order_release);
            liveTailRemainingSamples.store(std::max<std::int64_t>(1, pluginTailSamples),
                                           std::memory_order_release);
        }
    }

    // The audio thread only dereferences these pointers; the last reference is
    // always released on the control side when a graph is destroyed.
    std::shared_ptr<InstrumentBlock> instrumentBlock = std::make_shared<InstrumentBlock>();
    std::shared_ptr<PluginChain> effectChain = std::make_shared<PluginChain>();
    std::atomic<bool> liveMidiActiveState{false};
    std::atomic<bool> lowLatencyMonitoringState{false};
    std::atomic<bool> lowLatencyPdcBypassActiveState{false};
    std::atomic<int> heldNotes{0};
    std::atomic<bool> sustain{false};
    std::atomic<std::int64_t> liveTailRemainingSamples{0};
    juce::AudioBuffer<float> timelineInputDelayBuffer;
    int timelineInputDelaySamples = 0;
    int timelineInputDelayWritePosition = 0;
    bool timelineInputDelayActive = false;
};

}  // namespace riffra
