#include "SafetyAudioCallback.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <thread>
#include <utility>

namespace riffra {
SafetyAudioCallback::~SafetyAudioCallback() {
    juce::String ignored;
    if (timelineEngine != nullptr) stopArrangeRecording(*timelineEngine, ignored);
}

void SafetyAudioCallback::setTimelineEngine(TimelineEngine* const engine) noexcept {
    timelineEngine = engine;
}

SafetyAudioCallback::PreviewControlGuard::PreviewControlGuard(SafetyAudioCallback& ownerIn) noexcept
    : owner(ownerIn) {
    while (owner.previewBusy.test_and_set(std::memory_order_acquire)) std::this_thread::yield();
}

SafetyAudioCallback::PreviewControlGuard::~PreviewControlGuard() {
    owner.previewBusy.clear(std::memory_order_release);
}

SafetyAudioCallback::PreviewAudioGuard::PreviewAudioGuard(SafetyAudioCallback& ownerIn) noexcept
    : owner(ownerIn), ownsLock(!owner.previewBusy.test_and_set(std::memory_order_acquire)) {}

SafetyAudioCallback::PreviewAudioGuard::~PreviewAudioGuard() {
    if (ownsLock) owner.previewBusy.clear(std::memory_order_release);
}

namespace {

constexpr std::uint32_t muteReasonBit(const MuteReason reason) noexcept {
    return static_cast<std::uint32_t>(reason);
}

}  // namespace

void SafetyAudioCallback::setMuteReason(const MuteReason reason, const bool active) noexcept {
    const auto bit = muteReasonBit(reason);
    if (active) {
        muteReasons.fetch_or(bit, std::memory_order_acq_rel);
        resetGainOnNextCallback.store(true, std::memory_order_release);
        panicRequested.store(true, std::memory_order_release);
        synthPanicRequested.store(true, std::memory_order_release);
    } else {
        muteReasons.fetch_and(~bit, std::memory_order_acq_rel);
        resetGainOnNextCallback.store(true, std::memory_order_release);
    }
}

void SafetyAudioCallback::setUserEmergencyMute(const bool shouldMute) noexcept {
    setMuteReason(MuteReason::UserEmergency, shouldMute);
}

void SafetyAudioCallback::setEngineTransitionMute(const bool active) noexcept {
    setMuteReason(MuteReason::EngineTransition, active);
}

void SafetyAudioCallback::setFeedbackProtection(const bool active) noexcept {
    setMuteReason(MuteReason::FeedbackProtection, active);
    feedbackSuspected.store(active, std::memory_order_release);
    if (!active) feedbackDetector.reset();
}

std::uint32_t SafetyAudioCallback::getMuteReasons() const noexcept {
    return muteReasons.load(std::memory_order_acquire);
}

bool SafetyAudioCallback::isMuted() const noexcept { return getMuteReasons() != 0; }

bool SafetyAudioCallback::hasMuteReason(const MuteReason reason) const noexcept {
    return (getMuteReasons() & muteReasonBit(reason)) != 0;
}

void SafetyAudioCallback::setDeviceFaulted(const bool faulted) noexcept {
    setMuteReason(MuteReason::DeviceFault, faulted);
}

bool SafetyAudioCallback::isDeviceFaulted() const noexcept {
    return hasMuteReason(MuteReason::DeviceFault);
}

void SafetyAudioCallback::setDeviceTransitionActive(const bool active) noexcept {
    deviceTransitionActive.store(active, std::memory_order_release);
}

bool SafetyAudioCallback::isDeviceTransitionActive() const noexcept {
    return deviceTransitionActive.load(std::memory_order_acquire);
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
    while (value > current && !peak.compare_exchange_weak(current, value, std::memory_order_release,
                                                          std::memory_order_relaxed)) {
    }
}

std::uint64_t SafetyAudioCallback::getInvalidSampleCount() const noexcept {
    return invalidSamples.load(std::memory_order_acquire);
}

std::uint64_t SafetyAudioCallback::getCallbackCount() const noexcept {
    return callbackCount.load(std::memory_order_acquire);
}

std::uint64_t SafetyAudioCallback::getAverageCallbackDurationUs() const noexcept {
    const auto count = getCallbackCount();
    return count == 0 ? 0 : callbackDurationUs.load(std::memory_order_acquire) / count;
}

std::uint64_t SafetyAudioCallback::getMaximumCallbackDurationUs() const noexcept {
    return maximumCallbackDurationUs.load(std::memory_order_acquire);
}

std::uint64_t SafetyAudioCallback::getCallbackOverruns() const noexcept {
    return callbackOverruns.load(std::memory_order_acquire);
}

float SafetyAudioCallback::getPreLimiterPeak() const noexcept {
    return preLimiterPeak.exchange(0.0f, std::memory_order_acq_rel);
}

float SafetyAudioCallback::getLimiterGainReductionDb() const noexcept {
    return limiterGainReductionDb.exchange(0.0f, std::memory_order_acq_rel);
}

std::uint64_t SafetyAudioCallback::getHardClipSamples() const noexcept {
    return hardClipSamples.load(std::memory_order_acquire);
}

void SafetyAudioCallback::recordCallbackDuration(
    const std::chrono::steady_clock::time_point started, const int numSamples) noexcept {
    const auto duration = std::chrono::duration_cast<std::chrono::microseconds>(
                              std::chrono::steady_clock::now() - started)
                              .count();
    const auto durationUs = static_cast<std::uint64_t>(std::max<std::int64_t>(0, duration));
    callbackCount.fetch_add(1, std::memory_order_relaxed);
    callbackDurationUs.fetch_add(durationUs, std::memory_order_relaxed);
    auto maximum = maximumCallbackDurationUs.load(std::memory_order_relaxed);
    while (durationUs > maximum &&
           !maximumCallbackDurationUs.compare_exchange_weak(
               maximum, durationUs, std::memory_order_release, std::memory_order_relaxed)) {
    }
    const auto sampleRate = activeSampleRate.load(std::memory_order_relaxed);
    if (sampleRate > 0.0 && numSamples > 0 &&
        static_cast<double>(durationUs) >
            1'000'000.0 * static_cast<double>(numSamples) / sampleRate)
        callbackOverruns.fetch_add(1, std::memory_order_relaxed);
}

bool SafetyAudioCallback::isFeedbackSuspected() const noexcept {
    return feedbackSuspected.load(std::memory_order_acquire);
}

double SafetyAudioCallback::getSampleRate() const noexcept {
    return activeSampleRate.load(std::memory_order_acquire);
}

bool SafetyAudioCallback::startArrangeRecording(const juce::File& directory,
                                                TimelineEngine& timeline, juce::String& error) {
    const juce::ScopedLock lock(recordingLock);
    if (arrangeRecording != nullptr || pendingFinalization != nullptr || recordingProcessing) {
        error = "A recording is already active.";
        return false;
    }
    auto candidate =
        ArrangeRecordingSession::create(directory, timeline.recordingConfiguration(), error);
    if (candidate == nullptr) return false;
    arrangeRecording = std::move(candidate);
    arrangeRecordingCancelled.store(false, std::memory_order_release);
    recordingFinalizationStatus = juce::var{};
    timeline.setRecordingSink(arrangeRecording.get());
    return true;
}

void SafetyAudioCallback::setRecordingFinalizationDispatcher(
    RecordingFinalizationDispatcher dispatcher) {
    const juce::ScopedLock lock(recordingLock);
    recordingFinalizationDispatcher = std::move(dispatcher);
}

bool SafetyAudioCallback::stopArrangeRecording(TimelineEngine& timeline, juce::String& error) {
    std::unique_ptr<ArrangeRecordingSession> detached;
    RecordingFinalizationDispatcher dispatcher;
    {
        const juce::ScopedLock lock(recordingLock);
        if (recordingProcessing) {
            error = "The previous recording is still being processed.";
            return false;
        }
        if (pendingFinalization != nullptr) {
            error = "The previous recording is waiting for finalization.";
            return false;
        }
        timeline.stopRecording();
        const auto captureFinalized = timeline.finalizeRecording(error);
        timeline.stop();
        if (!captureFinalized) return false;
        timeline.clearRecordingSink();
        if (arrangeRecording == nullptr) return true;

        detached = std::move(arrangeRecording);
        recordingProcessing = true;
        recordingFinalizationStatus = detached->status();
        if (auto* status = recordingFinalizationStatus.getDynamicObject()) {
            status->setProperty("active", false);
            status->setProperty("processing", true);
        }
        dispatcher = recordingFinalizationDispatcher;
    }

    if (dispatcher != nullptr)
        dispatcher(std::move(detached));
    else {
        const juce::ScopedLock lock(recordingLock);
        pendingFinalization = std::move(detached);
    }
    return true;
}

std::unique_ptr<ArrangeRecordingSession> SafetyAudioCallback::takeFinalizedRecording() noexcept {
    const juce::ScopedLock lock(recordingLock);
    return std::move(pendingFinalization);
}

void SafetyAudioCallback::completeArrangeRecordingProcessing(const juce::var& status,
                                                             const juce::String& error) {
    const juce::ScopedLock lock(recordingLock);
    recordingFinalizationStatus = status;
    if (auto* result = recordingFinalizationStatus.getDynamicObject()) {
        result->setProperty("active", false);
        result->setProperty("processing", false);
        if (error.isNotEmpty()) result->setProperty("error", error);
    }
    recordingProcessing = false;
}

bool SafetyAudioCallback::cancelArrangeRecording(TimelineEngine& timeline, juce::String& error) {
    const juce::ScopedLock lock(recordingLock);
    if (recordingProcessing || pendingFinalization != nullptr) {
        error = "The previous recording is still being processed.";
        return false;
    }
    timeline.clearRecordingSink();
    if (arrangeRecording == nullptr) {
        arrangeRecordingCancelled.store(true, std::memory_order_release);
        return true;
    }
    auto cancelling = std::move(arrangeRecording);
    const auto cancelled = cancelling->cancel(error);
    arrangeRecordingCancelled.store(cancelled, std::memory_order_release);
    return cancelled;
}

juce::var SafetyAudioCallback::recordingStatus() const {
    const juce::ScopedLock lock(recordingLock);
    if (arrangeRecording != nullptr) {
        auto status = arrangeRecording->status();
        if (auto* result = status.getDynamicObject()) result->setProperty("processing", false);
        return status;
    }
    if (recordingFinalizationStatus.isObject()) return recordingFinalizationStatus;
    auto* status = new juce::DynamicObject();
    status->setProperty("active", false);
    status->setProperty("processing", false);
    status->setProperty("cancelled", arrangeRecordingCancelled.load(std::memory_order_acquire));
    return juce::var(status);
}

bool SafetyAudioCallback::startPreview(juce::AudioBuffer<float>& buffer, const int startSample,
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
        for (auto& voice : previewVoices) {
            if (voice.sequence < target->sequence) target = &voice;
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

void SafetyAudioCallback::stopPreviewForKey(const int voiceKey) noexcept {
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

void SafetyAudioCallback::startSynthNote(const int note, const float velocity) noexcept {
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

void SafetyAudioCallback::stopSynthNote(const int note) noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& voice : synthVoices) {
        if (voice.active && voice.note == note) voice.releasing = true;
    }
}

void SafetyAudioCallback::allNotesOff() noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& voice : synthVoices) {
        voice.releasing = true;
    }
}

bool SafetyAudioCallback::isPreviewing() const noexcept {
    const PreviewControlGuard lock(const_cast<SafetyAudioCallback&>(*this));
    for (const auto& voice : previewVoices) {
        if (voice.active) return true;
    }
    return false;
}

void SafetyAudioCallback::mixPreview(float* const* outputChannelData, const int numOutputChannels,
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

void SafetyAudioCallback::mixSynth(float* const* outputChannelData, const int numOutputChannels,
                                   const int numSamples) noexcept {
    if (synthPanicRequested.exchange(false, std::memory_order_acq_rel)) {
        for (auto& voice : synthVoices) voice.releasing = true;
    }
    const auto sampleRate = activeSampleRate.load(std::memory_order_acquire);
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
            for (int channel = 0; channel < numOutputChannels; ++channel) {
                if (outputChannelData[channel] != nullptr)
                    outputChannelData[channel][sample] += value;
            }
        }
    }
}

void SafetyAudioCallback::silenceAndCommit(float* const* outputChannelData,
                                           const int numOutputChannels, const int numSamples,
                                           const float rawInputPeak) noexcept {
    for (int channel = 0; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
    holdPeak(inputPeak, rawInputPeak);
    outputPeak.store(0.0f, std::memory_order_release);
}

bool SafetyAudioCallback::switchPreviewBuffer(const int voiceKey,
                                              const juce::AudioBuffer<float>& buffer,
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

void SafetyAudioCallback::audioDeviceIOCallbackWithContext(
    const float* const* inputChannelData, const int numInputChannels,
    float* const* outputChannelData, const int numOutputChannels, const int numSamples,
    const juce::AudioIODeviceCallbackContext&) {
    juce::ScopedNoDenormals noDenormals;
    const auto callbackStarted = std::chrono::steady_clock::now();
    const auto recordDuration = [this, callbackStarted, numSamples] {
        recordCallbackDuration(callbackStarted, numSamples);
    };
    if (panicRequested.exchange(false, std::memory_order_acq_rel) && timelineEngine != nullptr)
        timelineEngine->panicAllInstrumentTracks();
    if (timelineEngine != nullptr) timelineEngine->servicePendingPanic();
    if (timelineEngine != nullptr) {
        int recordingOffset = 0;
        int recordedSamples = numSamples;
        (void)timelineEngine->recordingWindow(numSamples, recordingOffset, recordedSamples);
    }
    const auto selectedChannel = inputChannel.load(std::memory_order_acquire);
    const auto* selectedInput = inputChannelData != nullptr && selectedChannel < numInputChannels
                                    ? inputChannelData[selectedChannel]
                                    : nullptr;
    const auto monitoringRoutesActive =
        timelineEngine != nullptr && timelineEngine->monitoringEnabled();
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
            if (!timelineEngine->monitoringInputChannel(channel)) continue;
            monitoringActive = true;
            monitoredInputPeak = std::max(monitoredInputPeak,
                                          peakForInput(inputChannelData[channel],
                                                       inputChannelData[channel] != selectedInput));
        }
    }
    if (invalidInputSamples > 0)
        invalidSamples.fetch_add(invalidInputSamples, std::memory_order_relaxed);

    const auto activeMuteReasons = getMuteReasons();
    if (activeMuteReasons != 0u) {
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        const auto graphMayRunWhileMuted =
            (activeMuteReasons & ~muteReasonBit(MuteReason::UserEmergency)) == 0u;
        if (graphMayRunWhileMuted && timelineEngine != nullptr)
            timelineEngine->mix(inputChannelData, numInputChannels, outputChannelData,
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

    if (timelineEngine != nullptr)
        timelineEngine->mix(inputChannelData, numInputChannels, outputChannelData,
                            numOutputChannels, numSamples);

    if (timelineEngine != nullptr)
        timelineEngine->mixMetronome(outputChannelData, numOutputChannels, numSamples);

    const PreviewAudioGuard previewTry(*this);
    if (previewTry.acquired()) {
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

    holdPeak(inputPeak, rawInputPeak);
    holdPeak(preLimiterPeak, blockPreLimiterPeak);
    holdPeak(outputPeak, blockOutputPeak);
    if (blockPreLimiterPeak > 0.0f && blockOutputPeak > 0.0f) {
        const auto reductionDb = juce::Decibels::gainToDecibels(
            juce::jmax(0.000001f, blockPreLimiterPeak / blockOutputPeak));
        holdPeak(limiterGainReductionDb, juce::jmax(0.0f, reductionDb));
    }
    if (blockHardClipSamples > 0)
        hardClipSamples.fetch_add(blockHardClipSamples, std::memory_order_relaxed);
    if (blockInvalidSamples > 0)
        invalidSamples.fetch_add(blockInvalidSamples, std::memory_order_relaxed);
    recordDuration();
}

void SafetyAudioCallback::audioDeviceAboutToStart(juce::AudioIODevice* const device) {
    // Touch the sine LUT on the main thread so its lazy initialization cannot
    // land inside the realtime audio callback.
    lookupSine(0.0f);
    const auto sampleRate = device != nullptr ? device->getCurrentSampleRate() : 0.0;
    activeSampleRate.store(sampleRate, std::memory_order_release);
    currentGainLinear = 0.0f;
    resetGainOnNextCallback.store(true, std::memory_order_release);
    fadeStep = sampleRate > 0.0 ? static_cast<float>(1.0 / (sampleRate * kFadeInSeconds)) : 0.0f;
    inputPeak.store(0.0f, std::memory_order_release);
    outputPeak.store(0.0f, std::memory_order_release);
    preLimiterPeak.store(0.0f, std::memory_order_release);
    limiterGainReductionDb.store(0.0f, std::memory_order_release);
    hardClipSamples.store(0, std::memory_order_release);
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
    if (timelineEngine != nullptr) timelineEngine->audioDeviceStarted();
}

void SafetyAudioCallback::audioDeviceStopped() {
    activeSampleRate.store(0.0, std::memory_order_release);
    limiterPrepared = false;
    currentGainLinear = 0.0f;
    inputPeak.store(0.0f, std::memory_order_release);
    outputPeak.store(0.0f, std::memory_order_release);
    preLimiterPeak.store(0.0f, std::memory_order_release);
    limiterGainReductionDb.store(0.0f, std::memory_order_release);
    dcBlocker.reset();
    feedbackDetector.reset();
    stopPreview();
    allNotesOff();
    juce::String ignored;
    if (timelineEngine != nullptr) stopArrangeRecording(*timelineEngine, ignored);
}

void SafetyAudioCallback::audioDeviceError(const juce::String& errorMessage) {
    const juce::ScopedLock lock(errorLock);
    lastDeviceError = errorMessage;
    setMuteReason(MuteReason::DeviceFault, true);
}

juce::String SafetyAudioCallback::takeLastDeviceError() {
    const juce::ScopedLock lock(errorLock);
    return std::exchange(lastDeviceError, {});
}

float SafetyAudioCallback::lookupSine(float phase) noexcept {
    static const std::array<float, kSineLUTSize + 1> lut = []() {
        std::array<float, kSineLUTSize + 1> values;
        for (int i = 0; i <= kSineLUTSize; ++i) {
            const double p = kTwoPi * static_cast<double>(i) / static_cast<double>(kSineLUTSize);
            values[i] = static_cast<float>(std::sin(p));
        }
        return values;
    }();
    // voice.phase is wrapped into [0, 2π) by the caller; the modulo guards the
    // last ULP so the interpolation read can never run past the table.
    const auto scaled = phase * static_cast<float>(kSineLUTSize / kTwoPi);
    const int i0 = static_cast<int>(scaled) % kSineLUTSize;
    const auto frac = scaled - std::floor(scaled);
    return lut[i0] + (lut[i0 + 1] - lut[i0]) * frac;
}

}  // namespace riffra
