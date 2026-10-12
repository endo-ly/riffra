#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <vector>

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
    }

    void add(const juce::AudioBuffer<float>& source, juce::AudioBuffer<float>& destination,
             const int sampleCount, const float gain, const bool audible,
             const bool lowLatency) noexcept {
        if (delaySamples == 0) {
            if (audible)
                for (int channel = 0; channel < 2; ++channel)
                    juce::FloatVectorOperations::addWithMultiply(
                        destination.getWritePointer(channel), source.getReadPointer(channel), gain,
                        sampleCount);
            return;
        }
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto read = (position + 1) % buffer.getNumSamples();
            for (int channel = 0; channel < 2; ++channel) {
                const auto value = audible ? source.getSample(channel, sample) * gain : 0.0f;
                buffer.setSample(channel, position, value);
                destination.addSample(channel, sample,
                                      lowLatency ? value : buffer.getSample(channel, read));
            }
            position = read;
        }
    }

private:
    juce::AudioBuffer<float> buffer;
    int delaySamples = 0;
    int position = 0;
};

/// Prepared effect processing and mix controls shared by Groups and Returns.
struct MixerChannel final {
    juce::String id;
    std::vector<PluginDeviceSpec> effectSpecs;
    std::shared_ptr<PluginChain> effects = std::make_shared<PluginChain>();
    juce::AudioBuffer<float> input;
    juce::AudioBuffer<float> output;
    std::atomic<float> gainDb{0.0f};
    std::atomic<float> pan{0.0f};
    bool unityCenterStereo = true;
    bool audible = true;
    std::int64_t inputLatency = 0;
    std::int64_t outputLatency = 0;
    TrackMeterAccumulator meter;
    std::vector<TrackRuntime*> sources;

    [[nodiscard]] bool lowLatency() const noexcept {
        return std::any_of(sources.begin(), sources.end(),
                           [](const auto* source) { return source->lowLatencyMonitoring(); });
    }

    void process(const int sampleCount) noexcept {
        effects->process(input.getArrayOfReadPointers(), 2, output.getArrayOfWritePointers(), 2,
                         sampleCount);
        const auto angle =
            (pan.load(std::memory_order_acquire) + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const auto gain = juce::Decibels::decibelsToGain(gainDb.load(std::memory_order_acquire));
        const auto scale = unityCenterStereo ? std::sqrt(2.0f) : 1.0f;
        output.applyGain(0, 0, sampleCount,
                         audible ? gain * std::min(1.0f, std::cos(angle) * scale) : 0.0f);
        output.applyGain(1, 0, sampleCount,
                         audible ? gain * std::min(1.0f, std::sin(angle) * scale) : 0.0f);
        meter.recordBlock(
            output.getMagnitude(0, 0, sampleCount), output.getMagnitude(1, 0, sampleCount),
            output.getRMSLevel(0, 0, sampleCount), output.getRMSLevel(1, 0, sampleCount));
    }
};

/// One Track or Bus output entering a Group, Return, or Master.
struct MixerRoute final {
    juce::String sendId;
    TrackRuntime* track = nullptr;
    MixerChannel* bus = nullptr;
    juce::AudioBuffer<float>* source = nullptr;
    juce::AudioBuffer<float>* destination = nullptr;
    std::atomic<float> gain{1.0f};
    bool audible = true;
    MixRouteDelay delay;

    void mix(const int sampleCount) noexcept {
        delay.add(*source, *destination, sampleCount, gain.load(std::memory_order_acquire), audible,
                  track != nullptr ? track->lowLatencyMonitoring() : bus->lowLatency());
    }
};

/// Immutable routing with prepared buffers and reusable plugin instances.
struct MixerRuntime final {
    std::vector<std::unique_ptr<MixerChannel>> buses;
    std::vector<MixerChannel*> parallelChannels;
    std::vector<std::unique_ptr<MixerRoute>> trackRoutes;
    std::vector<std::unique_ptr<MixerRoute>> busRoutes;
    std::vector<PluginDeviceSpec> masterEffectSpecs;
    std::shared_ptr<PluginChain> masterEffects = std::make_shared<PluginChain>();
    juce::AudioBuffer<float> masterInput;
    juce::AudioBuffer<float> masterOutput;
    std::int64_t masterInputLatency = 0;

    [[nodiscard]] MixerChannel* findBus(const juce::String& id) const noexcept {
        for (const auto& bus : buses)
            if (bus->id == id) return bus.get();
        return nullptr;
    }

    void reset() noexcept {
        masterInput.clear();
        masterOutput.clear();
        masterEffects->allNotesOff();
        for (auto& bus : buses) {
            bus->input.clear();
            bus->output.clear();
            bus->effects->allNotesOff();
        }
        for (auto& route : trackRoutes) route->delay.reset();
        for (auto& route : busRoutes) route->delay.reset();
    }
};

}  // namespace riffra
