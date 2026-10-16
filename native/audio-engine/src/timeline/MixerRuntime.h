#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>
#include <vector>

#include "MixAutomation.h"
#include "TrackRuntime.h"
#include "contract/ExecutionGraph.h"
#include "plugins/PluginChain.h"

namespace riffra {

/// Prepared stereo delay used on every edge entering a mixing junction.
class MixRouteDelay final {
public:
    void prepare(const int samples) {
        delaySamples = samples;
        buffer.setSize(2, samples + 1);
        reset();
    }

    void reset() noexcept {
        buffer.clear();
        position = 0;
        lowLatencyActive = false;
    }

    void add(const juce::AudioBuffer<float>& source, juce::AudioBuffer<float>& destination,
             const int sampleCount, const float gain, const bool audible, const bool lowLatency,
             AutomationRuntime::Cursor* automation = nullptr,
             const std::int64_t rangeStart = 0) noexcept {
        if (lowLatency != lowLatencyActive) {
            buffer.clear();
            position = 0;
            lowLatencyActive = lowLatency;
        }
        if (delaySamples == 0 && automation == nullptr) {
            if (audible)
                for (int channel = 0; channel < 2; ++channel)
                    juce::FloatVectorOperations::addWithMultiply(
                        destination.getWritePointer(channel), source.getReadPointer(channel), gain,
                        sampleCount);
            return;
        }
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto read = (position + 1) % buffer.getNumSamples();
            const auto sampleGain =
                automation != nullptr
                    ? juce::Decibels::decibelsToGain(automation->valueAt(rangeStart + sample, 0.0f))
                    : gain;
            for (int channel = 0; channel < 2; ++channel) {
                const auto value = audible ? source.getSample(channel, sample) * sampleGain : 0.0f;
                buffer.setSample(channel, position, value);
                destination.addSample(
                    channel, sample,
                    lowLatency || delaySamples == 0 ? value : buffer.getSample(channel, read));
            }
            position = read;
        }
    }

private:
    juce::AudioBuffer<float> buffer;
    int delaySamples = 0;
    int position = 0;
    bool lowLatencyActive = false;
};

/// Prepared effect processing and mix controls shared by Groups and Returns.
struct MixerChannel final {
    juce::String id;
    std::vector<PluginDeviceSpec> effectSpecs;
    std::vector<PluginParameterAutomation> deviceAutomation;
    std::shared_ptr<PluginChain> effects = std::make_shared<PluginChain>();
    juce::AudioBuffer<float> input;
    juce::AudioBuffer<float> output;
    std::atomic<float> gainDb{0.0f};
    std::atomic<float> pan{0.0f};
    AutomationPreviewValue gainPreview;
    AutomationPreviewValue panPreview;
    AutomationRuntime gainAutomation;
    AutomationRuntime panAutomation;
    bool unityCenterStereo = true;
    bool audible = true;
    bool hasInput = false;
    bool hasAudibleInput = false;
    bool hasNonLowLatencyInput = false;
    std::vector<TrackRuntime*> lowLatencySources;
    std::int64_t inputLatency = 0;
    std::int64_t outputLatency = 0;
    TrackMeterAccumulator meter;
    [[nodiscard]] bool lowLatency() const noexcept {
        return hasInput && !hasNonLowLatencyInput && !lowLatencySources.empty() &&
               std::all_of(lowLatencySources.begin(), lowLatencySources.end(),
                           [](const auto* source) { return source->lowLatencyMonitoring(); });
    }

    void process(const int sampleCount, const std::int64_t rangeStart,
                 const bool playing) noexcept {
        effects->process(input.getArrayOfReadPointers(), 2, output.getArrayOfWritePointers(), 2,
                         sampleCount, nullptr, deviceAutomation, rangeStart - inputLatency,
                         playing);
        if (playing && (!gainAutomation.empty() || !panAutomation.empty())) {
            MixAutomationCursor cursor(gainAutomation, panAutomation, rangeStart - outputLatency,
                                       sampleCount, gainDb.load(std::memory_order_acquire),
                                       pan.load(std::memory_order_acquire), unityCenterStereo, true,
                                       gainPreview.current(), panPreview.current());
            for (int sample = 0; sample < sampleCount; ++sample) {
                const auto gains = cursor.next();
                output.setSample(0, sample,
                                 audible ? output.getSample(0, sample) * gains.left : 0.0f);
                output.setSample(1, sample,
                                 audible ? output.getSample(1, sample) * gains.right : 0.0f);
            }
        } else {
            const auto angle = (pan.load(std::memory_order_acquire) + 1.0f) *
                               juce::MathConstants<float>::pi * 0.25f;
            const auto gain =
                juce::Decibels::decibelsToGain(gainDb.load(std::memory_order_acquire));
            const auto scale = unityCenterStereo ? std::sqrt(2.0f) : 1.0f;
            output.applyGain(0, 0, sampleCount,
                             audible ? gain * std::min(1.0f, std::cos(angle) * scale) : 0.0f);
            output.applyGain(1, 0, sampleCount,
                             audible ? gain * std::min(1.0f, std::sin(angle) * scale) : 0.0f);
        }
        meter.recordBlock(
            output.getMagnitude(0, 0, sampleCount), output.getMagnitude(1, 0, sampleCount),
            output.getRMSLevel(0, 0, sampleCount), output.getRMSLevel(1, 0, sampleCount));
    }
};

/// One Track or Bus output entering a Group, Return, or Master.
struct MixerRoute final {
    juce::String sendId;
    TrackRuntime* track = nullptr;
    MixerChannel* destinationBus = nullptr;
    MixerChannel* sourceBus = nullptr;
    juce::AudioBuffer<float>* source = nullptr;
    juce::AudioBuffer<float>* destination = nullptr;
    std::atomic<float> gain{1.0f};
    bool audible = true;
    bool allowLowLatencyBypass = false;
    AutomationRuntime levelAutomation;
    AutomationPreviewValue levelPreview;
    MixRouteDelay delay;

    void mix(const int sampleCount, const std::int64_t rangeStart, const bool playing) noexcept {
        const auto lowLatency = sourceBus != nullptr ? sourceBus->lowLatency()
                                                     : allowLowLatencyBypass && track != nullptr &&
                                                           track->lowLatencyPdcBypassActive();
        const auto latency = sourceBus != nullptr ? sourceBus->outputLatency
                             : track != nullptr   ? track->routeLatencySamples
                                                  : 0;
        auto cursor = levelAutomation.cursorAt(rangeStart - latency);
        const auto preview = levelPreview.current();
        const auto routeGain = preview ? juce::Decibels::decibelsToGain(*preview)
                                       : gain.load(std::memory_order_acquire);
        delay.add(*source, *destination, sampleCount, routeGain, audible, lowLatency,
                  playing && !preview && !levelAutomation.empty() ? &cursor : nullptr,
                  rangeStart - latency);
    }
};

/// Immutable routing with prepared buffers and reusable plugin instances.
struct MixerRuntime final {
    std::vector<std::unique_ptr<MixerChannel>> buses;
    std::vector<MixerChannel*> parallelChannels;
    std::vector<std::unique_ptr<MixerRoute>> trackRoutes;
    std::vector<std::unique_ptr<MixerRoute>> busRoutes;
    std::vector<std::unique_ptr<MixerRoute>> sidechainRoutes;
    /// Sidechain routes grouped by the stage whose output they carry.
    std::vector<std::vector<MixerRoute*>> sidechainRoutesByStage;
    std::vector<PluginDeviceSpec> masterEffectSpecs;
    std::vector<PluginParameterAutomation> masterDeviceAutomation;
    std::shared_ptr<PluginChain> masterEffects = std::make_shared<PluginChain>();
    juce::AudioBuffer<float> masterInput;
    juce::AudioBuffer<float> masterOutput;
    std::int64_t masterInputLatency = 0;
    AutomationRuntime masterGainAutomation;
    AutomationPreviewValue masterGainPreview;

    [[nodiscard]] MixerChannel* findBus(const juce::String& id) const noexcept {
        for (const auto& bus : buses)
            if (bus->id == id) return bus.get();
        return nullptr;
    }

    void reset() noexcept {
        masterInput.clear();
        masterOutput.clear();
        masterEffects->allNotesOff();
        masterEffects->resetSidechainState();
        for (auto& bus : buses) {
            bus->input.clear();
            bus->output.clear();
            bus->effects->allNotesOff();
            bus->effects->resetSidechainState();
        }
        for (auto& route : trackRoutes) route->delay.reset();
        for (auto& route : busRoutes) route->delay.reset();
        for (auto& route : sidechainRoutes) route->delay.reset();
    }
};

}  // namespace riffra
