#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>

#include "PluginChain.h"
#include "instrument/InstrumentRuntime.h"

namespace riffra {

/// The stage at which a prepared audio take enters the track graph.
enum class ProcessingStage { PreEffects, PostEffects };

/// One realtime Track graph. Timeline, live, and monitoring input all use the
/// same instrument and effect chain owned here.
class TrackRuntime final {
public:
    TrackRuntime() = default;
    ~TrackRuntime() = default;

    TrackRuntime(const TrackRuntime&) = delete;
    TrackRuntime& operator=(const TrackRuntime&) = delete;
    TrackRuntime(TrackRuntime&&) noexcept = delete;
    TrackRuntime& operator=(TrackRuntime&&) noexcept = delete;

    [[nodiscard]] InstrumentRuntime* instrument() noexcept { return instrumentRuntime.get(); }
    [[nodiscard]] const InstrumentRuntime* instrument() const noexcept {
        return instrumentRuntime.get();
    }
    [[nodiscard]] PluginChain& effects() noexcept { return effectChain; }
    [[nodiscard]] const PluginChain& effects() const noexcept { return effectChain; }

    void setInstrument(std::unique_ptr<InstrumentRuntime> runtime) noexcept {
        instrumentRuntime = std::move(runtime);
    }

    [[nodiscard]] bool hasLoadedInstrument() const noexcept {
        return instrumentRuntime != nullptr && instrumentRuntime->isLoaded();
    }

    [[nodiscard]] bool enqueueMidi(const juce::MidiMessage& message) noexcept {
        if (instrumentRuntime == nullptr || !instrumentRuntime->enqueueMidi(message)) return false;
        updateLiveActivity(message);
        liveMidiActiveState.store(true, std::memory_order_release);
        return true;
    }

    void panic() noexcept {
        if (instrumentRuntime != nullptr) instrumentRuntime->allNotesOff();
        effectChain.allNotesOff();
        heldNotes.store(0, std::memory_order_release);
        sustain.store(false, std::memory_order_release);
        liveTailRemainingSamples.store(
            std::max(1, instrumentRuntime != nullptr ? instrumentRuntime->tailSamples() : 0),
            std::memory_order_release);
        liveMidiActiveState.store(false, std::memory_order_release);
    }

    void resetForTransportDiscontinuity() noexcept {
        if (instrumentRuntime != nullptr) instrumentRuntime->resetForTransportDiscontinuity();
        effectChain.allNotesOff();
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
        return effectChain.latencySamples() +
               (instrumentRuntime != nullptr ? instrumentRuntime->latencySamples() : 0);
    }

    [[nodiscard]] int pluginTailSamples() const noexcept {
        return effectChain.tailSamples() +
               (instrumentRuntime != nullptr ? instrumentRuntime->tailSamples() : 0);
    }

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
                liveTailRemainingSamples.store(
                    std::max(1,
                             instrumentRuntime != nullptr ? instrumentRuntime->tailSamples() : 0),
                    std::memory_order_release);
            return;
        }
        if (message.isController() && message.getControllerNumber() == 64) {
            const auto isDown = message.getControllerValue() >= 64;
            sustain.store(isDown, std::memory_order_release);
            if (!isDown && heldNotes.load(std::memory_order_relaxed) == 0)
                liveTailRemainingSamples.store(
                    std::max(1,
                             instrumentRuntime != nullptr ? instrumentRuntime->tailSamples() : 0),
                    std::memory_order_release);
            return;
        }
        if (message.isAllNotesOff() || message.isAllSoundOff()) {
            heldNotes.store(0, std::memory_order_release);
            sustain.store(false, std::memory_order_release);
            liveTailRemainingSamples.store(
                std::max(1, instrumentRuntime != nullptr ? instrumentRuntime->tailSamples() : 0),
                std::memory_order_release);
        }
    }

    std::unique_ptr<InstrumentRuntime> instrumentRuntime;
    PluginChain effectChain;
    std::atomic<bool> liveMidiActiveState{false};
    std::atomic<int> heldNotes{0};
    std::atomic<bool> sustain{false};
    std::atomic<std::int64_t> liveTailRemainingSamples{0};
};

}  // namespace riffra
