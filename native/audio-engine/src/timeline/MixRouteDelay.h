#pragma once
#include <JuceHeader.h>

#include "AutomationRuntime.h"
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

}  // namespace riffra
