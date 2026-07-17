#include "SafetyAudioCallback.h"

#include <array>
#include <algorithm>
#include <cmath>

namespace riffra {
SafetyAudioCallback::~SafetyAudioCallback() {
    juce::String ignored;
    stopRecording(ignored);
}

void SafetyAudioCallback::setPluginRack(PluginRack* const rack) noexcept {
    pluginRack = rack;
}


void SafetyAudioCallback::setEmergencyMuted(const bool shouldMute) noexcept {
    if (shouldMute)
        allNotesOff();
    emergencyMuted.store(shouldMute, std::memory_order_release);
    if (!shouldMute) {
        currentGainLinear = 0.0f;
        feedbackSuspected.store(false, std::memory_order_release);
    }
}

bool SafetyAudioCallback::isEmergencyMuted() const noexcept {
    return emergencyMuted.load(std::memory_order_acquire);
}

void SafetyAudioCallback::setDeviceFaulted(const bool faulted) noexcept {
    deviceFaulted.store(faulted, std::memory_order_release);
}

bool SafetyAudioCallback::isDeviceFaulted() const noexcept {
    return deviceFaulted.load(std::memory_order_acquire);
}

void SafetyAudioCallback::setMasterGainDb(const float gainDb) noexcept {
    const auto safeGain = juce::jlimit(kMinimumGainDb, kMaximumGainDb, gainDb);
    masterGainDb.store(safeGain, std::memory_order_release);
    targetGainLinear.store(juce::Decibels::decibelsToGain(safeGain), std::memory_order_release);
}

float SafetyAudioCallback::getMasterGainDb() const noexcept {
    return masterGainDb.load(std::memory_order_acquire);
}

void SafetyAudioCallback::setInputChannel(const int channel) noexcept {
    inputChannel.store(juce::jmax(0, channel), std::memory_order_release);
}

int SafetyAudioCallback::getInputChannel() const noexcept {
    return inputChannel.load(std::memory_order_acquire);
}

float SafetyAudioCallback::getInputPeak() const noexcept {
    return inputPeak.exchange(0.0f, std::memory_order_acq_rel);
}

float SafetyAudioCallback::getOutputPeak() const noexcept {
    return outputPeak.exchange(0.0f, std::memory_order_acq_rel);
}

void SafetyAudioCallback::holdPeak(std::atomic<float>& peak, const float value) noexcept {
    auto current = peak.load(std::memory_order_relaxed);
    while (value > current
        && !peak.compare_exchange_weak(
            current,
            value,
            std::memory_order_release,
            std::memory_order_relaxed)) {
    }
}

std::uint64_t SafetyAudioCallback::getInvalidSampleCount() const noexcept {
    return invalidSamples.load(std::memory_order_acquire);
}

bool SafetyAudioCallback::isFeedbackSuspected() const noexcept {
    return feedbackSuspected.load(std::memory_order_acquire);
}

double SafetyAudioCallback::getSampleRate() const noexcept {
    return activeSampleRate.load(std::memory_order_acquire);
}

bool SafetyAudioCallback::startRecording(
    const juce::File& directory,
    juce::String& error) {
    const juce::ScopedLock lock(recordingLock);
    if (recording != nullptr) {
        error = "A recording is already active.";
        return false;
    }
    const auto sampleRate = activeSampleRate.load(std::memory_order_acquire);
    const auto rawChannels = activeInputChannels.load(std::memory_order_acquire);
    const auto processedChannels = activeOutputChannels.load(std::memory_order_acquire);
    if (rawChannels <= 0) {
        error = "No active input channel is available for Raw recording.";
        return false;
    }
    if (rawChannels > 32 || processedChannels <= 0 || processedChannels > 32) {
        error = "Recording currently supports between 1 and 32 raw and processed channels.";
        return false;
    }
    auto candidate = RecordingSession::create(
        directory,
        sampleRate,
        rawChannels,
        processedChannels,
        error);
    if (candidate == nullptr)
        return false;
    recording = std::move(candidate);
    activeRecording.store(recording.get(), std::memory_order_release);
    return true;
}

bool SafetyAudioCallback::stopRecording(juce::String& error) {
    const juce::ScopedLock lock(recordingLock);
    activeRecording.store(nullptr, std::memory_order_release);
    while (recordingReaders.load(std::memory_order_acquire) != 0)
        juce::Thread::sleep(1);
    if (recording == nullptr)
        return true;
    auto finishing = std::move(recording);
    return finishing->finish(error);
}

juce::var SafetyAudioCallback::recordingStatus() const {
    const juce::ScopedLock lock(recordingLock);
    if (recording != nullptr)
        return recording->status();
    auto* status = new juce::DynamicObject();
    status->setProperty("active", false);
    return juce::var(status);
}

bool SafetyAudioCallback::startPreview(
    juce::AudioBuffer<float>& buffer,
    const int startSample,
    const int endSample,
    const float gain,
    const bool loop,
    juce::String& error,
    const int voiceKey) {
    const juce::ScopedLock lock(previewLock);
    if (buffer.getNumChannels() <= 0 || buffer.getNumSamples() <= 0) {
        error = "Preview source contains no audio samples.";
        return false;
    }
    const auto safeStart = juce::jlimit(0, buffer.getNumSamples() - 1, startSample);
    const auto safeEnd = juce::jlimit(safeStart + 1, buffer.getNumSamples(), endSample);
    if (safeEnd <= safeStart) {
        error = "Preview range is empty.";
        return false;
    }
    PreviewVoice* target = nullptr;
    if (voiceKey >= 0) {
        for (auto& voice : previewVoices) {
            if (voice.active && voice.key == voiceKey) {
                target = &voice;
                break;
            }
        }
    }
    if (target == nullptr) {
        for (auto& voice : previewVoices) {
            if (!voice.active) {
                target = &voice;
                break;
            }
        }
    }
    if (target == nullptr) {
        target = &previewVoices.front();
        for (auto& voice : previewVoices) {
            if (voice.sequence < target->sequence)
                target = &voice;
        }
    }
    target->buffer.makeCopyOf(buffer, true);
    target->key = voiceKey;
    target->start = safeStart;
    target->cursor = safeStart;
    target->end = safeEnd;
    target->gain = juce::jlimit(0.0f, 2.0f, gain);
    target->loop = loop;
    target->active = true;
    target->sequence = ++previewSequence;
    return true;
}

void SafetyAudioCallback::stopPreview() noexcept {
    const juce::ScopedLock lock(previewLock);
    for (auto& voice : previewVoices) {
        voice.active = false;
        voice.key = -1;
        voice.start = 0;
        voice.cursor = 0;
        voice.end = 0;
        voice.loop = false;
        voice.buffer.setSize(0, 0);
    }
}

void SafetyAudioCallback::stopPreviewForKey(const int voiceKey) noexcept {
    const juce::ScopedLock lock(previewLock);
    for (auto& voice : previewVoices) {
        if (voice.active && voice.key == voiceKey) {
            voice.active = false;
            voice.key = -1;
            voice.cursor = voice.start;
            voice.loop = false;
        }
    }
}

void SafetyAudioCallback::startSynthNote(const int note, const float velocity) noexcept {
    if (note < 0 || note > 127)
        return;
    const juce::ScopedLock lock(previewLock);
    SynthVoice* target = nullptr;
    for (auto& voice : synthVoices) {
        if (voice.active && voice.note == note) {
            target = &voice;
            break;
        }
    }
    if (target == nullptr) {
        for (auto& voice : synthVoices) {
            if (!voice.active) {
                target = &voice;
                break;
            }
        }
    }
    if (target == nullptr)
        target = &synthVoices.front();
    target->note = note;
    target->phase = 0.0f;
    target->level = 0.0f;
    target->targetLevel = juce::jlimit(0.02f, 0.18f, velocity) * 0.8f;
    target->active = true;
    target->releasing = false;
}

void SafetyAudioCallback::stopSynthNote(const int note) noexcept {
    const juce::ScopedLock lock(previewLock);
    for (auto& voice : synthVoices) {
        if (voice.active && voice.note == note)
            voice.releasing = true;
    }
}

void SafetyAudioCallback::allNotesOff() noexcept {
    const juce::ScopedLock lock(previewLock);
    for (auto& voice : synthVoices) {
        voice.releasing = true;
    }
}

bool SafetyAudioCallback::isPreviewing() const noexcept {
    const juce::ScopedLock lock(previewLock);
    for (const auto& voice : previewVoices) {
        if (voice.active)
            return true;
    }
    return false;
}

void SafetyAudioCallback::mixPreview(
    float* const* outputChannelData,
    const int numOutputChannels,
    const int numSamples) noexcept {
    for (auto& voice : previewVoices) {
        if (!voice.active || voice.buffer.getNumSamples() <= 0)
            continue;
        const auto sourceChannels = voice.buffer.getNumChannels();
        for (int sample = 0; sample < numSamples && voice.active; ++sample) {
            if (voice.cursor >= voice.end) {
                if (voice.loop)
                    voice.cursor = voice.start;
                else {
                    voice.active = false;
                    break;
                }
            }
            for (int channel = 0; channel < numOutputChannels; ++channel) {
                auto* output = outputChannelData[channel];
                if (output == nullptr)
                    continue;
                const auto sourceChannel = juce::jmin(channel, sourceChannels - 1);
                output[sample] += voice.buffer.getSample(sourceChannel, voice.cursor) * voice.gain;
            }
            ++voice.cursor;
        }
    }
}

void SafetyAudioCallback::mixSynth(
    float* const* outputChannelData,
    const int numOutputChannels,
    const int numSamples) noexcept {
    const auto sampleRate = activeSampleRate.load(std::memory_order_acquire);
    if (sampleRate <= 0.0 || numOutputChannels <= 0)
        return;
    constexpr float twoPi = 6.2831853071795864769f;
    for (auto& voice : synthVoices) {
        if (!voice.active)
            continue;
        const auto frequency = 440.0 * std::pow(2.0, (static_cast<double>(voice.note) - 69.0) / 12.0);
        const auto phaseStep = static_cast<float>(twoPi * frequency / sampleRate);
        for (int sample = 0; sample < numSamples && voice.active; ++sample) {
            if (voice.releasing) {
                voice.level *= 0.995f;
                if (voice.level < 0.0001f) {
                    voice.active = false;
                    break;
                }
            } else {
                voice.level = std::min(voice.targetLevel, voice.level + 0.004f);
            }
            const auto value = std::sin(voice.phase) * voice.level;
            voice.phase += phaseStep;
            if (voice.phase >= twoPi)
                voice.phase -= twoPi;
            for (int channel = 0; channel < numOutputChannels; ++channel) {
                if (outputChannelData[channel] != nullptr)
                    outputChannelData[channel][sample] += value;
            }
        }
    }
}

void SafetyAudioCallback::writeRecording(
    const float* const* inputChannelData,
    const int numInputChannels,
    float* const* outputChannelData,
    const int numOutputChannels,
    const int numSamples) noexcept {
    recordingReaders.fetch_add(1, std::memory_order_acq_rel);
    auto* session = activeRecording.load(std::memory_order_acquire);
    if (session != nullptr && numSamples <= silenceBuffer.getNumSamples()) {
        std::array<const float*, 32> raw {};
        std::array<const float*, 32> processed {};
        const auto* silence = silenceBuffer.getReadPointer(0);
        for (int channel = 0; channel < session->getRawChannels(); ++channel) {
            raw[static_cast<std::size_t>(channel)] =
                channel < numInputChannels && inputChannelData[channel] != nullptr
                    ? inputChannelData[channel]
                    : silence;
        }
        for (int channel = 0; channel < session->getProcessedChannels(); ++channel) {
            processed[static_cast<std::size_t>(channel)] =
                channel < numOutputChannels && outputChannelData[channel] != nullptr
                    ? outputChannelData[channel]
                    : silence;
        }
        session->write(raw.data(), processed.data(), numSamples);
    }
    recordingReaders.fetch_sub(1, std::memory_order_acq_rel);
}

void SafetyAudioCallback::audioDeviceIOCallbackWithContext(
    const float* const* inputChannelData,
    const int numInputChannels,
    float* const* outputChannelData,
    const int numOutputChannels,
    const int numSamples,
    const juce::AudioIODeviceCallbackContext&) {
    const auto selectedChannel = inputChannel.load(std::memory_order_acquire);
    const auto* selectedInput = selectedChannel < numInputChannels
        ? inputChannelData[selectedChannel]
        : nullptr;
    const std::array<const float*, 1> logicalInputs { selectedInput };
    const auto numLogicalInputs = selectedInput != nullptr ? 1 : 0;
    float rawInputPeak = 0.0f;
    if (selectedInput != nullptr) {
        const auto maxVal = std::abs(juce::FloatVectorOperations::findMaximum(
            selectedInput, numSamples));
        const auto minVal = std::abs(juce::FloatVectorOperations::findMinimum(
            selectedInput, numSamples));
        rawInputPeak = std::max({rawInputPeak, maxVal, minVal});
    }

    if (emergencyMuted.load(std::memory_order_acquire)) {
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        holdPeak(inputPeak, rawInputPeak);
        outputPeak.store(0.0f, std::memory_order_release);
        writeRecording(
            logicalInputs.data(),
            numLogicalInputs,
            outputChannelData,
            numOutputChannels,
            numSamples);
        return;
    }

    feedbackDetector.observe(rawInputPeak, numSamples);
    if (feedbackDetector.consumeSuspected()) {
        emergencyMuted.store(true, std::memory_order_release);
        feedbackSuspected.store(true, std::memory_order_release);
        allNotesOff();
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        holdPeak(inputPeak, rawInputPeak);
        outputPeak.store(0.0f, std::memory_order_release);
        writeRecording(
            logicalInputs.data(),
            numLogicalInputs,
            outputChannelData,
            numOutputChannels,
            numSamples);
        return;
    }

    const auto target = targetGainLinear.load(std::memory_order_acquire);
    float blockOutputPeak = 0.0f;
    std::uint64_t blockInvalidSamples = 0;

    if (pluginRack != nullptr) {
        pluginRack->process(
            logicalInputs.data(),
            numLogicalInputs,
            outputChannelData,
            numOutputChannels,
            numSamples);
    } else {
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr) {
                if (selectedInput != nullptr)
                    juce::FloatVectorOperations::copy(
                        outputChannelData[channel], selectedInput, numSamples);
                else
                    juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
            }
    }

    const juce::ScopedTryLock previewTry(previewLock);
    if (previewTry.isLocked()) {
        mixPreview(outputChannelData, numOutputChannels, numSamples);
        mixSynth(outputChannelData, numOutputChannels, numSamples);
    }

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
            value = juce::jlimit(-kLimiterCeiling, kLimiterCeiling, value);
            blockOutputPeak = std::max(blockOutputPeak, std::abs(value));
            if (outputChannelData[channel] != nullptr)
                outputChannelData[channel][sample] = value;
        }
    }

    holdPeak(inputPeak, rawInputPeak);
    holdPeak(outputPeak, blockOutputPeak);
    if (blockInvalidSamples > 0)
        invalidSamples.fetch_add(blockInvalidSamples, std::memory_order_relaxed);
    writeRecording(
        logicalInputs.data(),
        numLogicalInputs,
        outputChannelData,
        numOutputChannels,
        numSamples);
}

void SafetyAudioCallback::audioDeviceAboutToStart(juce::AudioIODevice* const device) {
    const auto sampleRate = device != nullptr ? device->getCurrentSampleRate() : 0.0;
    activeSampleRate.store(sampleRate, std::memory_order_release);
    currentGainLinear = 0.0f;
    fadeStep = sampleRate > 0.0
        ? static_cast<float>(1.0 / (sampleRate * kFadeInSeconds))
        : 0.0f;
    inputPeak.store(0.0f, std::memory_order_release);
    activeInputChannels.store(
        device != nullptr
                && getInputChannel() < device->getActiveInputChannels().countNumberOfSetBits()
            ? 1
            : 0,
        std::memory_order_release);
    activeOutputChannels.store(
        device != nullptr ? device->getActiveOutputChannels().countNumberOfSetBits() : 0,
        std::memory_order_release);
    outputPeak.store(0.0f, std::memory_order_release);
    silenceBuffer.setSize(1, device != nullptr ? juce::jmax(1, device->getCurrentBufferSizeSamples()) : 1);
    silenceBuffer.clear();
    dcBlocker.prepare(device != nullptr
        ? static_cast<int>(device->getActiveOutputChannels().countNumberOfSetBits())
        : 0);
    feedbackDetector.prepare(sampleRate);
    feedbackSuspected.store(false, std::memory_order_release);
    if (pluginRack != nullptr)
        pluginRack->prepare(activeSampleRate.load(std::memory_order_acquire), silenceBuffer.getNumSamples());
}

void SafetyAudioCallback::audioDeviceStopped() {
    activeSampleRate.store(0.0, std::memory_order_release);
    currentGainLinear = 0.0f;
    inputPeak.store(0.0f, std::memory_order_release);
    outputPeak.store(0.0f, std::memory_order_release);
    dcBlocker.reset();
    feedbackDetector.reset();
    if (pluginRack != nullptr)
        pluginRack->release();
    stopPreview();
    allNotesOff();
    juce::String ignored;
    stopRecording(ignored);
    activeInputChannels.store(0, std::memory_order_release);
    activeOutputChannels.store(0, std::memory_order_release);
}

void SafetyAudioCallback::audioDeviceError(const juce::String& errorMessage) {
    const juce::ScopedLock lock(errorLock);
    lastDeviceError = errorMessage;
    emergencyMuted.store(true, std::memory_order_release);
}

juce::String SafetyAudioCallback::takeLastDeviceError() {
    const juce::ScopedLock lock(errorLock);
    return std::exchange(lastDeviceError, {});
}

} // namespace riffra
