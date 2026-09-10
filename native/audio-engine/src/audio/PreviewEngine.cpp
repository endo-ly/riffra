#include "PreviewEngine.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <thread>

namespace riffra {

PreviewEngine::PreviewControlGuard::PreviewControlGuard(PreviewEngine& ownerIn) noexcept
    : owner(ownerIn) {
    while (owner.previewBusy.test_and_set(std::memory_order_acquire)) std::this_thread::yield();
}

PreviewEngine::PreviewControlGuard::~PreviewControlGuard() {
    owner.previewBusy.clear(std::memory_order_release);
}

PreviewEngine::PreviewAudioGuard::PreviewAudioGuard(PreviewEngine& ownerIn) noexcept
    : owner(ownerIn), ownsLock(!owner.previewBusy.test_and_set(std::memory_order_acquire)) {}

PreviewEngine::PreviewAudioGuard::~PreviewAudioGuard() {
    if (ownsLock) owner.previewBusy.clear(std::memory_order_release);
}

bool PreviewEngine::startPreview(juce::AudioBuffer<float>& buffer, const int startSample,
                                 const int endSample, const float gain, const bool loop,
                                 juce::String& error, const int voiceKey) {
    const PreviewControlGuard lock(*this);
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
        for (auto& voice : previewVoices)
            if (voice.sequence < target->sequence) target = &voice;
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

void PreviewEngine::stopPreview() noexcept {
    const PreviewControlGuard lock(*this);
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

void PreviewEngine::stopPreviewForKey(const int voiceKey) noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& voice : previewVoices) {
        if (voice.active && voice.key == voiceKey) {
            voice.active = false;
            voice.key = -1;
            voice.cursor = voice.start;
            voice.loop = false;
        }
    }
}

void PreviewEngine::startSynthNote(const int note, const float velocity) noexcept {
    if (note < 0 || note > 127) return;
    const PreviewControlGuard lock(*this);
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
    if (target == nullptr) target = &synthVoices.front();
    target->note = note;
    target->frequency = 440.0f * std::pow(2.0f, (static_cast<float>(note) - 69.0f) / 12.0f);
    target->phase = 0.0f;
    target->level = 0.0f;
    target->targetLevel = juce::jlimit(0.02f, 0.18f, velocity) * 0.8f;
    target->active = true;
    target->releasing = false;
}

void PreviewEngine::stopSynthNote(const int note) noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& voice : synthVoices)
        if (voice.active && voice.note == note) voice.releasing = true;
}

void PreviewEngine::allNotesOff() noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& voice : synthVoices) voice.releasing = true;
}

bool PreviewEngine::isPreviewing() const noexcept {
    const PreviewControlGuard lock(const_cast<PreviewEngine&>(*this));
    for (const auto& voice : previewVoices)
        if (voice.active) return true;
    return false;
}

void PreviewEngine::prepare() noexcept { (void)lookupSine(0.0f); }

bool PreviewEngine::tryMix(float* const* outputChannelData, const int numOutputChannels,
                           const int numSamples, const double sampleRate) noexcept {
    const PreviewAudioGuard previewTry(*this);
    if (!previewTry.acquired()) return false;
    mixPreview(outputChannelData, numOutputChannels, numSamples);
    mixSynth(outputChannelData, numOutputChannels, numSamples, sampleRate);
    return true;
}

void PreviewEngine::requestSynthPanic() noexcept {
    synthPanicRequested.store(true, std::memory_order_release);
}

void PreviewEngine::mixPreview(float* const* outputChannelData, const int numOutputChannels,
                               const int numSamples) noexcept {
    for (auto& voice : previewVoices) {
        if (!voice.active || voice.buffer.getNumSamples() <= 0) continue;
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
                if (output == nullptr) continue;
                const auto sourceChannel = juce::jmin(channel, sourceChannels - 1);
                output[sample] += voice.buffer.getSample(sourceChannel, voice.cursor) * voice.gain;
            }
            ++voice.cursor;
        }
    }
}

void PreviewEngine::mixSynth(float* const* outputChannelData, const int numOutputChannels,
                             const int numSamples, const double sampleRate) noexcept {
    if (synthPanicRequested.exchange(false, std::memory_order_acq_rel))
        for (auto& voice : synthVoices) voice.releasing = true;
    if (sampleRate <= 0.0 || numOutputChannels <= 0) return;
    constexpr float twoPi = static_cast<float>(kTwoPi);
    for (auto& voice : synthVoices) {
        if (!voice.active) continue;
        const auto phaseStep = static_cast<float>(twoPi * voice.frequency / sampleRate);
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
            const auto value = lookupSine(voice.phase) * voice.level;
            voice.phase += phaseStep;
            if (voice.phase >= twoPi) voice.phase -= twoPi;
            for (int channel = 0; channel < numOutputChannels; ++channel)
                if (outputChannelData[channel] != nullptr)
                    outputChannelData[channel][sample] += value;
        }
    }
}

bool PreviewEngine::switchPreviewBuffer(const int voiceKey, const juce::AudioBuffer<float>& buffer,
                                        juce::String& error) {
    const PreviewControlGuard lock(*this);
    if (buffer.getNumChannels() <= 0 || buffer.getNumSamples() <= 0) {
        error = "Take comparison source contains no audio.";
        return false;
    }
    for (auto& voice : previewVoices) {
        if (!voice.active || voice.key != voiceKey) continue;
        const auto relativeCursor = std::max(0, voice.cursor - voice.start);
        voice.buffer.makeCopyOf(buffer, true);
        voice.start = 0;
        voice.end = buffer.getNumSamples();
        voice.cursor = std::min(relativeCursor, voice.end - 1);
        voice.sequence = ++previewSequence;
        return true;
    }
    error = "Take comparison is not active.";
    return false;
}

float PreviewEngine::lookupSine(float phase) noexcept {
    static const std::array<float, kSineLUTSize + 1> lut = []() {
        std::array<float, kSineLUTSize + 1> values;
        for (int i = 0; i <= kSineLUTSize; ++i) {
            const double p = kTwoPi * static_cast<double>(i) / static_cast<double>(kSineLUTSize);
            values[i] = static_cast<float>(std::sin(p));
        }
        return values;
    }();
    const auto scaled = phase * static_cast<float>(kSineLUTSize / kTwoPi);
    const int i0 = static_cast<int>(scaled) % kSineLUTSize;
    const auto frac = scaled - std::floor(scaled);
    return lut[i0] + (lut[i0 + 1] - lut[i0]) * frac;
}

}  // namespace riffra
