#include "AudioRenderPipeline.h"

#include <algorithm>
#include <cmath>

#include "PreviewEngine.h"
#include "processing/TimelineEngine.h"

namespace riffra {

AudioRenderPipeline::AudioRenderPipeline(TimelineEngine& timelineIn) noexcept
    : timelineEngine(timelineIn), recordingController(timelineIn) {}

AudioRenderPipeline::~AudioRenderPipeline() = default;

void AudioRenderPipeline::setMuteReason(const MuteReason reason, const bool active) noexcept {
    const auto bit = muteReasonBit(reason);
    if (active) {
        muteReasons.fetch_or(bit, std::memory_order_acq_rel);
        resetGainOnNextCallback.store(true, std::memory_order_release);
        panicRequested.store(true, std::memory_order_release);
        previewEngine.requestSynthPanic();
    } else {
        muteReasons.fetch_and(~bit, std::memory_order_acq_rel);
        resetGainOnNextCallback.store(true, std::memory_order_release);
    }
}

std::uint32_t AudioRenderPipeline::muteReasonBit(const MuteReason reason) noexcept {
    return static_cast<std::uint32_t>(reason);
}

void AudioRenderPipeline::setUserEmergencyMute(const bool shouldMute) noexcept {
    setMuteReason(MuteReason::UserEmergency, shouldMute);
}

void AudioRenderPipeline::setEngineTransitionMute(const bool active) noexcept {
    setMuteReason(MuteReason::EngineTransition, active);
}

void AudioRenderPipeline::setFeedbackProtection(const bool active) noexcept {
    setMuteReason(MuteReason::FeedbackProtection, active);
    feedbackSuspected.store(active, std::memory_order_release);
    if (!active) feedbackDetector.reset();
}

std::uint32_t AudioRenderPipeline::getMuteReasons() const noexcept {
    return muteReasons.load(std::memory_order_acquire);
}

bool AudioRenderPipeline::isMuted() const noexcept { return getMuteReasons() != 0; }

bool AudioRenderPipeline::hasMuteReason(const MuteReason reason) const noexcept {
    return (getMuteReasons() & muteReasonBit(reason)) != 0;
}

void AudioRenderPipeline::setDeviceFaulted(const bool faulted) noexcept {
    setMuteReason(MuteReason::DeviceFault, faulted);
}

bool AudioRenderPipeline::isDeviceFaulted() const noexcept {
    return hasMuteReason(MuteReason::DeviceFault);
}

bool AudioRenderPipeline::isPreviewing() const noexcept { return previewEngine.isPreviewing(); }

void AudioRenderPipeline::setMasterGainDb(const float gainDb) noexcept {
    const auto safeGain = juce::jlimit(kMinimumGainDb, kMaximumGainDb, gainDb);
    masterGainDb.store(safeGain, std::memory_order_release);
    targetGainLinear.store(juce::Decibels::decibelsToGain(safeGain), std::memory_order_release);
}

float AudioRenderPipeline::getMasterGainDb() const noexcept {
    return masterGainDb.load(std::memory_order_acquire);
}

void AudioRenderPipeline::setInputChannel(const int channel) noexcept {
    inputChannel.store(juce::jmax(0, channel), std::memory_order_release);
}

int AudioRenderPipeline::getInputChannel() const noexcept {
    return inputChannel.load(std::memory_order_acquire);
}

double AudioRenderPipeline::getSampleRate() const noexcept {
    return activeSampleRate.load(std::memory_order_acquire);
}

void AudioRenderPipeline::silenceAndCommit(float* const* outputChannelData,
                                           const int numOutputChannels, const int numSamples,
                                           const float rawInputPeak) noexcept {
    for (int channel = 0; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
    audioMetrics.recordSilencedBlock(rawInputPeak);
}

void AudioRenderPipeline::processBlock(const float* const* inputChannelData,
                                       const int numInputChannels, float* const* outputChannelData,
                                       const int numOutputChannels, const int numSamples,
                                       const juce::AudioIODeviceCallbackContext&) noexcept {
    juce::ScopedNoDenormals noDenormals;
    const auto callbackStarted = std::chrono::steady_clock::now();
    const auto recordDuration = [this, callbackStarted, numSamples] {
        audioMetrics.recordCallbackDuration(callbackStarted, numSamples,
                                            activeSampleRate.load(std::memory_order_relaxed));
    };
    if (panicRequested.exchange(false, std::memory_order_acq_rel))
        timelineEngine.panicAllInstrumentTracks();
    timelineEngine.servicePendingPanic();
    int recordingOffset = 0;
    int recordedSamples = numSamples;
    (void)timelineEngine.recordingWindow(numSamples, recordingOffset, recordedSamples);

    const auto selectedChannel = inputChannel.load(std::memory_order_acquire);
    const auto* selectedInput = inputChannelData != nullptr && selectedChannel < numInputChannels
                                    ? inputChannelData[selectedChannel]
                                    : nullptr;
    const auto monitoringRoutesActive = timelineEngine.monitoringEnabled();
    std::uint64_t invalidInputSamples = 0;
    const auto peakForInput = [numSamples, &invalidInputSamples](const float* const input,
                                                                 const bool countInvalid) noexcept {
        if (input == nullptr || numSamples <= 0) return 0.0f;
        float peak = 0.0f;
        for (int sample = 0; sample < numSamples; ++sample) {
            const auto value = input[sample];
            if (!std::isfinite(value)) {
                if (countInvalid) ++invalidInputSamples;
                continue;
            }
            peak = std::max(peak, std::abs(value));
        }
        return peak;
    };
    const auto rawInputPeak = peakForInput(selectedInput, true);
    float monitoredInputPeak = 0.0f;
    bool monitoringActive = false;
    if (monitoringRoutesActive && inputChannelData != nullptr) {
        for (int channel = 0; channel < numInputChannels; ++channel) {
            if (!timelineEngine.monitoringInputChannel(channel)) continue;
            monitoringActive = true;
            monitoredInputPeak = std::max(monitoredInputPeak,
                                          peakForInput(inputChannelData[channel],
                                                       inputChannelData[channel] != selectedInput));
        }
    }
    if (invalidInputSamples > 0)
        audioMetrics.recordBlock(0.0f, 0.0f, 0.0f, 0.0f, 0, invalidInputSamples);

    const auto activeMuteReasons = getMuteReasons();
    if (activeMuteReasons != 0u) {
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        const auto graphMayRunWhileMuted =
            (activeMuteReasons & ~muteReasonBit(MuteReason::UserEmergency)) == 0u;
        if (graphMayRunWhileMuted)
            timelineEngine.mix(inputChannelData, numInputChannels, outputChannelData,
                               numOutputChannels, numSamples);
        silenceAndCommit(outputChannelData, numOutputChannels, numSamples, rawInputPeak);
        recordDuration();
        return;
    }

    feedbackDetector.observe(monitoredInputPeak, numSamples, monitoringActive);
    if (feedbackDetector.consumeSuspected()) {
        setFeedbackProtection(true);
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        silenceAndCommit(outputChannelData, numOutputChannels, numSamples, rawInputPeak);
        recordDuration();
        return;
    }

    const auto target = targetGainLinear.load(std::memory_order_acquire);
    if (resetGainOnNextCallback.exchange(false, std::memory_order_acq_rel))
        currentGainLinear = 0.0f;
    float blockPreLimiterPeak = 0.0f;
    float blockOutputPeak = 0.0f;
    std::uint64_t blockInvalidSamples = 0;

    for (int channel = 0; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);

    timelineEngine.mix(inputChannelData, numInputChannels, outputChannelData, numOutputChannels,
                       numSamples);
    timelineEngine.mixMetronome(outputChannelData, numOutputChannels, numSamples);
    (void)previewEngine.tryMix(outputChannelData, numOutputChannels, numSamples,
                               activeSampleRate.load(std::memory_order_acquire));
    dcBlocker.processBlock(outputChannelData, numOutputChannels, numSamples);

    for (int sample = 0; sample < numSamples; ++sample) {
        if (currentGainLinear < target)
            currentGainLinear = std::min(target, currentGainLinear + fadeStep);
        else
            currentGainLinear = target;
        for (int channel = 0; channel < numOutputChannels; ++channel) {
            const auto* input = outputChannelData[channel];
            auto value = input != nullptr ? input[sample] : 0.0f;
            if (!std::isfinite(value)) {
                value = 0.0f;
                ++blockInvalidSamples;
            }
            value *= currentGainLinear;
            blockPreLimiterPeak = std::max(blockPreLimiterPeak, std::abs(value));
            if (outputChannelData[channel] != nullptr) outputChannelData[channel][sample] = value;
        }
    }

    bool limiterReady = limiterPrepared && numOutputChannels > 0 &&
                        numOutputChannels <= static_cast<int>(limiterChannels.size());
    if (limiterReady) {
        for (int channel = 0; channel < numOutputChannels; ++channel) {
            limiterChannels[static_cast<std::size_t>(channel)] = outputChannelData[channel];
            if (limiterChannels[static_cast<std::size_t>(channel)] == nullptr) {
                limiterReady = false;
                break;
            }
        }
    }
    if (limiterReady) {
        juce::dsp::AudioBlock<float> block(limiterChannels.data(),
                                           static_cast<std::size_t>(numOutputChannels),
                                           static_cast<std::size_t>(numSamples));
        limiter.process(juce::dsp::ProcessContextReplacing<float>(block));
    }

    std::uint64_t blockHardClipSamples = 0;
    for (int sample = 0; sample < numSamples; ++sample) {
        for (int channel = 0; channel < numOutputChannels; ++channel) {
            auto* output = outputChannelData[channel];
            if (output == nullptr) continue;
            auto value = output[sample];
            if (!std::isfinite(value)) {
                value = 0.0f;
                ++blockInvalidSamples;
            }
            if (std::abs(value) > kLimiterCeiling) ++blockHardClipSamples;
            value = juce::jlimit(-kLimiterCeiling, kLimiterCeiling, value);
            output[sample] = value;
            blockOutputPeak = std::max(blockOutputPeak, std::abs(value));
        }
    }

    float reductionDb = 0.0f;
    if (blockPreLimiterPeak > 0.0f && blockOutputPeak > 0.0f) {
        reductionDb = juce::jmax(0.0f, juce::Decibels::gainToDecibels(juce::jmax(
                                           0.000001f, blockPreLimiterPeak / blockOutputPeak)));
    }
    audioMetrics.recordBlock(rawInputPeak, blockPreLimiterPeak, blockOutputPeak, reductionDb,
                             blockHardClipSamples, blockInvalidSamples);
    recordDuration();
}

void AudioRenderPipeline::prepare(juce::AudioIODevice* const device) {
    // Touch the sine LUT on the message thread so lazy initialization cannot
    // land inside the realtime audio callback.
    previewEngine.prepare();
    const auto sampleRate = device != nullptr ? device->getCurrentSampleRate() : 0.0;
    activeSampleRate.store(sampleRate, std::memory_order_release);
    currentGainLinear = 0.0f;
    resetGainOnNextCallback.store(true, std::memory_order_release);
    fadeStep = sampleRate > 0.0 ? static_cast<float>(1.0 / (sampleRate * kFadeInSeconds)) : 0.0f;
    audioMetrics.resetForDevice();
    dcBlocker.prepare(
        device != nullptr
            ? static_cast<int>(device->getActiveOutputChannels().countNumberOfSetBits())
            : 0);
    feedbackDetector.prepare(sampleRate);
    const auto outputChannels =
        device != nullptr ? device->getActiveOutputChannels().countNumberOfSetBits() : 0;
    const auto blockSize = device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
    limiterPrepared = sampleRate > 0.0 && outputChannels > 0 && blockSize > 0;
    if (limiterPrepared) {
        limiter.prepare({sampleRate, static_cast<juce::uint32>(blockSize),
                         static_cast<juce::uint32>(outputChannels)});
        limiter.setThreshold(juce::Decibels::gainToDecibels(kLimiterCeiling));
        limiter.setRelease(50.0f);
        limiter.reset();
    }
    feedbackSuspected.store(false, std::memory_order_release);
    timelineEngine.audioDeviceStarted();
}

void AudioRenderPipeline::deviceStopped() noexcept {
    activeSampleRate.store(0.0, std::memory_order_release);
    limiterPrepared = false;
    currentGainLinear = 0.0f;
    audioMetrics.resetForDevice();
    dcBlocker.reset();
    feedbackDetector.reset();
    previewEngine.stopPreview();
    previewEngine.allNotesOff();
}

}  // namespace riffra
