#include "PreviewEngine.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <thread>
#include <utility>

#include "InstrumentPreviewSession.h"

namespace riffra {
namespace {

int fadeFrames(const double sampleRate) noexcept {
    constexpr double kFadeSeconds = 0.005;
    return std::max(1, static_cast<int>(std::ceil(std::max(1.0, sampleRate) * kFadeSeconds)));
}

}  // namespace

PreviewEngine::PreviewEngine() {
    for (auto& count : audioReaderCounts) count.store(0, std::memory_order_relaxed);
    auto initial = std::make_unique<PreviewState>();
    auto* initialState = initial.get();
    previewStates.push_back(std::move(initial));
    pendingPreviewState.store(initialState, std::memory_order_release);
    for (auto& state : audioVoiceStates) state.store(nullptr, std::memory_order_relaxed);
    for (auto& state : audioVoicePendingStates) state.store(nullptr, std::memory_order_relaxed);
    for (auto& cursor : audioVoiceCursors) cursor.store(0, std::memory_order_relaxed);
}

PreviewEngine::~PreviewEngine() = default;

PreviewEngine::PreviewControlGuard::PreviewControlGuard(PreviewEngine& ownerIn) noexcept
    : owner(ownerIn) {
    while (owner.previewBusy.test_and_set(std::memory_order_acquire)) std::this_thread::yield();
    owner.retireFinishedBuiltInState();
    owner.cleanupDeferredState();
}

PreviewEngine::PreviewControlGuard::~PreviewControlGuard() {
    owner.cleanupDeferredState();
    owner.previewBusy.clear(std::memory_order_release);
}

PreviewEngine::PreviewAudioGuard::PreviewAudioGuard(PreviewEngine& ownerIn) noexcept
    : owner(ownerIn) {
    for (;;) {
        generation = owner.audioReaderGeneration.load(std::memory_order_acquire);
        owner.audioReaderCounts[generation].fetch_add(1, std::memory_order_acq_rel);
        if (owner.audioReaderGeneration.load(std::memory_order_acquire) == generation) {
            entered = true;
            return;
        }
        owner.audioReaderCounts[generation].fetch_sub(1, std::memory_order_release);
    }
}

PreviewEngine::PreviewAudioGuard::~PreviewAudioGuard() {
    if (entered) owner.audioReaderCounts[generation].fetch_sub(1, std::memory_order_release);
}

void PreviewEngine::publishState(std::unique_ptr<PreviewState> next) {
    auto* state = next.get();
    previewStates.push_back(std::move(next));
    pendingPreviewState.store(state, std::memory_order_release);
}

void PreviewEngine::retireFinishedBuiltInState() {
    auto* state = pendingPreviewState.load(std::memory_order_acquire);
    if (state == nullptr || state->builtInSession == nullptr ||
        !state->builtInSession->isFinished())
        return;
    const auto* session = state->builtInSession;
    if (audioBuiltInSession.load(std::memory_order_acquire) == session ||
        audioBuiltInPendingSession.load(std::memory_order_acquire) == session)
        return;
    auto next = std::make_unique<PreviewState>(*state);
    next->builtInSession = nullptr;
    next->builtInBuffer = nullptr;
    publishState(std::move(next));
}

void PreviewEngine::cleanupDeferredState() noexcept {
    if (!deferredCleanupPending) {
        deferredCleanupGeneration = audioReaderGeneration.load(std::memory_order_acquire);
        audioReaderGeneration.store(1U - deferredCleanupGeneration, std::memory_order_release);
        deferredCleanupPending = true;
    }
    if (audioReaderCounts[deferredCleanupGeneration].load(std::memory_order_acquire) != 0) return;
    deferredCleanupPending = false;
    const auto activeGeneration = 1U - deferredCleanupGeneration;
    const auto shouldReclaim = [activeGeneration, this](auto& retired, const auto* candidate,
                                                        const bool needed) {
        if (needed) {
            retired.erase(candidate);
            return false;
        }
        const auto [it, inserted] = retired.emplace(candidate, activeGeneration);
        if (inserted || it->second != deferredCleanupGeneration) return false;
        retired.erase(it);
        return true;
    };
    const auto* pending = pendingPreviewState.load(std::memory_order_acquire);
    const auto* audio = audioPreviewState.load(std::memory_order_acquire);
    const auto* builtIn = audioBuiltInSession.load(std::memory_order_acquire);
    const auto* pendingBuiltIn = audioBuiltInPendingSession.load(std::memory_order_acquire);
    const auto* builtInBuffer = audioBuiltInBuffer.load(std::memory_order_acquire);
    const auto* pendingBuiltInBuffer = audioBuiltInPendingBuffer.load(std::memory_order_acquire);
    const auto stateIsNeeded = [this, pending, audio](const PreviewState* candidate) {
        if (candidate == pending || candidate == audio) return true;
        for (const auto& state : audioVoiceStates)
            if (state.load(std::memory_order_acquire) == candidate) return true;
        for (const auto& state : audioVoicePendingStates)
            if (state.load(std::memory_order_acquire) == candidate) return true;
        return false;
    };
    previewStates.erase(
        std::remove_if(
            previewStates.begin(), previewStates.end(),
            [this, &stateIsNeeded, &shouldReclaim](const std::unique_ptr<PreviewState>& state) {
                return shouldReclaim(retiredPreviewStates, state.get(), stateIsNeeded(state.get()));
            }),
        previewStates.end());

    const auto bufferIsNeeded = [this](const juce::AudioBuffer<float>* candidate) {
        for (const auto& state : previewStates)
            for (const auto& voice : state->voices)
                if (voice.buffer == candidate) return true;
        return false;
    };
    previewBuffers.erase(
        std::remove_if(previewBuffers.begin(), previewBuffers.end(),
                       [this, &bufferIsNeeded,
                        &shouldReclaim](const std::unique_ptr<juce::AudioBuffer<float>>& buffer) {
                           return shouldReclaim(retiredPreviewBuffers, buffer.get(),
                                                bufferIsNeeded(buffer.get()));
                       }),
        previewBuffers.end());

    const auto sessionIsNeeded = [this, builtIn,
                                  pendingBuiltIn](const InstrumentPreviewSession* candidate) {
        if (candidate == builtIn || candidate == pendingBuiltIn) return true;
        for (const auto& state : previewStates)
            if (state->builtInSession == candidate) return true;
        return false;
    };
    builtInSessions.erase(
        std::remove_if(builtInSessions.begin(), builtInSessions.end(),
                       [this, &sessionIsNeeded,
                        &shouldReclaim](const std::unique_ptr<InstrumentPreviewSession>& session) {
                           return shouldReclaim(retiredBuiltInSessions, session.get(),
                                                sessionIsNeeded(session.get()));
                       }),
        builtInSessions.end());

    const auto builtInBufferIsNeeded =
        [this, builtInBuffer, pendingBuiltInBuffer](const juce::AudioBuffer<float>* candidate) {
            if (candidate == builtInBuffer || candidate == pendingBuiltInBuffer) return true;
            for (const auto& state : previewStates)
                if (state->builtInBuffer == candidate) return true;
            return false;
        };
    builtInBuffers.erase(
        std::remove_if(builtInBuffers.begin(), builtInBuffers.end(),
                       [this, &builtInBufferIsNeeded,
                        &shouldReclaim](const std::unique_ptr<juce::AudioBuffer<float>>& buffer) {
                           return shouldReclaim(retiredBuiltInBuffers, buffer.get(),
                                                builtInBufferIsNeeded(buffer.get()));
                       }),
        builtInBuffers.end());
}

bool PreviewEngine::startPreview(juce::AudioBuffer<float>& buffer, const int startSample,
                                 const int endSample, const float gain, const bool loop,
                                 juce::String& error, const int voiceKey) {
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

    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    if (voiceKey < 0) {
        next->builtInSession = nullptr;
        next->builtInBuffer = nullptr;
        builtInStopRequested.store(true, std::memory_order_release);
    }

    std::size_t targetIndex = kPreviewVoiceCount;
    if (voiceKey >= 0) {
        for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
            const auto& voice = next->voices[index];
            if (voice.active && voice.key == voiceKey) {
                targetIndex = index;
                break;
            }
        }
    }
    if (targetIndex == kPreviewVoiceCount) {
        for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
            if (!next->voices[index].active) {
                targetIndex = index;
                break;
            }
        }
    }
    if (targetIndex == kPreviewVoiceCount) {
        targetIndex = 0;
        for (std::size_t index = 1; index < kPreviewVoiceCount; ++index)
            if (next->voices[index].revision < next->voices[targetIndex].revision)
                targetIndex = index;
    }

    auto ownedBuffer = std::make_unique<juce::AudioBuffer<float>>();
    ownedBuffer->makeCopyOf(buffer, true);
    const auto* source = ownedBuffer.get();
    previewBuffers.push_back(std::move(ownedBuffer));
    auto& target = next->voices[targetIndex];
    target.buffer = source;
    target.key = voiceKey;
    target.start = safeStart;
    target.cursor = safeStart;
    target.end = safeEnd;
    target.gain = juce::jlimit(0.0f, 2.0f, gain);
    target.loop = loop;
    target.active = true;
    target.revision = ++previewSequence;
    publishState(std::move(next));
    return true;
}

bool PreviewEngine::startBuiltInPreview(const juce::String& definitionJson,
                                        const juce::String& definitionBaseDir,
                                        InstrumentPreviewSpec spec, const double sampleRate,
                                        const int blockSize, juce::String& error) {
    auto session = InstrumentPreviewSession::create(definitionJson, definitionBaseDir,
                                                    std::move(spec), sampleRate, blockSize, error);
    if (session == nullptr) return false;

    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    for (auto& voice : next->voices) {
        if (!voice.active || voice.key == 1) continue;
        voice.active = false;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    builtInSessions.push_back(std::move(session));
    auto builtInBuffer = std::make_unique<juce::AudioBuffer<float>>();
    builtInBuffer->setSize(2, std::max(1, blockSize), false, true, true);
    auto* builtInBufferPtr = builtInBuffer.get();
    builtInBuffers.push_back(std::move(builtInBuffer));
    next->builtInSession = builtInSessions.back().get();
    next->builtInBuffer = builtInBufferPtr;
    builtInStopRequested.store(false, std::memory_order_release);
    publishState(std::move(next));
    return true;
}

void PreviewEngine::stopBuiltInPreview() noexcept {
    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    next->builtInSession = nullptr;
    next->builtInBuffer = nullptr;
    builtInStopRequested.store(true, std::memory_order_release);
    publishState(std::move(next));
}

void PreviewEngine::stopPreview() noexcept {
    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    next->builtInSession = nullptr;
    next->builtInBuffer = nullptr;
    for (auto& voice : next->voices) {
        voice.active = false;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    builtInStopRequested.store(true, std::memory_order_release);
    publishState(std::move(next));
}

void PreviewEngine::stopPreviewForKey(const int voiceKey) noexcept {
    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    for (auto& voice : next->voices) {
        if (!voice.active || voice.key != voiceKey) continue;
        voice.active = false;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    publishState(std::move(next));
}

bool PreviewEngine::switchPreviewBuffer(const int voiceKey, const juce::AudioBuffer<float>& buffer,
                                        juce::String& error) {
    if (buffer.getNumChannels() <= 0 || buffer.getNumSamples() <= 0) {
        error = "Take comparison source contains no audio.";
        return false;
    }
    const PreviewControlGuard lock(*this);
    auto next =
        std::make_unique<PreviewState>(*pendingPreviewState.load(std::memory_order_acquire));
    for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
        auto& voice = next->voices[index];
        if (!voice.active || voice.key != voiceKey) continue;
        auto ownedBuffer = std::make_unique<juce::AudioBuffer<float>>();
        ownedBuffer->makeCopyOf(buffer, true);
        const auto* source = ownedBuffer.get();
        previewBuffers.push_back(std::move(ownedBuffer));
        const auto relativeCursor =
            std::max(0, audioVoiceCursors[index].load(std::memory_order_acquire) - voice.start);
        voice.buffer = source;
        voice.start = 0;
        voice.end = buffer.getNumSamples();
        voice.cursor = std::min(relativeCursor, voice.end - 1);
        voice.revision = ++previewSequence;
        publishState(std::move(next));
        return true;
    }
    error = "Take comparison is not active.";
    return false;
}

void PreviewEngine::startSynthNote(const int note, const float velocity) noexcept {
    if (note < 0 || note > 127) return;
    const PreviewControlGuard lock(*this);
    std::size_t targetIndex = kSynthVoiceCount;
    for (std::size_t index = 0; index < kSynthVoiceCount; ++index) {
        if (synthControl[index].active.load(std::memory_order_acquire) &&
            synthControl[index].note.load(std::memory_order_acquire) == note) {
            targetIndex = index;
            break;
        }
    }
    if (targetIndex == kSynthVoiceCount) {
        for (std::size_t index = 0; index < kSynthVoiceCount; ++index) {
            const auto active = synthControl[index].active.load(std::memory_order_acquire);
            const auto finished =
                synthControl[index].audioFinishedRevision.load(std::memory_order_acquire) ==
                synthControl[index].revision.load(std::memory_order_acquire);
            if (!active || finished) {
                targetIndex = index;
                break;
            }
        }
    }
    if (targetIndex == kSynthVoiceCount) targetIndex = 0;
    auto& control = synthControl[targetIndex];
    control.active.store(false, std::memory_order_release);
    control.note.store(note, std::memory_order_relaxed);
    control.frequency.store(440.0f * std::pow(2.0f, (static_cast<float>(note) - 69.0f) / 12.0f),
                            std::memory_order_relaxed);
    control.targetLevel.store(juce::jlimit(0.02f, 0.18f, velocity) * 0.8f,
                              std::memory_order_relaxed);
    control.releasing.store(false, std::memory_order_relaxed);
    control.revision.fetch_add(1, std::memory_order_release);
    control.active.store(true, std::memory_order_release);
}

void PreviewEngine::stopSynthNote(const int note) noexcept {
    const PreviewControlGuard lock(*this);
    for (auto& control : synthControl)
        if (control.active.load(std::memory_order_acquire) &&
            control.note.load(std::memory_order_acquire) == note)
            control.releasing.store(true, std::memory_order_release);
}

void PreviewEngine::allNotesOff() noexcept {
    const PreviewControlGuard lock(*this);
    if (pendingPreviewState.load(std::memory_order_acquire)->builtInSession != nullptr)
        builtInStopRequested.store(true, std::memory_order_release);
    for (auto& control : synthControl) control.releasing.store(true, std::memory_order_release);
}

bool PreviewEngine::isPreviewing() const noexcept {
    const PreviewControlGuard lock(const_cast<PreviewEngine&>(*this));
    const auto* state = pendingPreviewState.load(std::memory_order_acquire);
    if (state != nullptr && state->builtInSession != nullptr &&
        !state->builtInSession->isFinished() &&
        !builtInStopRequested.load(std::memory_order_acquire))
        return true;
    if (state != nullptr)
        for (const auto& voice : state->voices)
            if (voice.active) return true;
    for (const auto& control : synthControl)
        if (control.active.load(std::memory_order_acquire) &&
            control.audioFinishedRevision.load(std::memory_order_acquire) !=
                control.revision.load(std::memory_order_acquire))
            return true;
    return false;
}

bool PreviewEngine::isBuiltInPreviewing() const noexcept {
    const PreviewControlGuard lock(const_cast<PreviewEngine&>(*this));
    const auto* state = pendingPreviewState.load(std::memory_order_acquire);
    return state != nullptr && state->builtInSession != nullptr &&
           !state->builtInSession->isFinished() &&
           !builtInStopRequested.load(std::memory_order_acquire);
}

void PreviewEngine::prepare() noexcept { (void)lookupSine(0.0f); }

void PreviewEngine::configureVoice(PreviewVoiceRuntime& voice, PreviewState* sourceState,
                                   const PreviewVoiceConfig& config,
                                   const double sampleRate) noexcept {
    voice.state = VoiceState::fadingIn;
    voice.sourceState = sourceState;
    voice.pendingState = nullptr;
    voice.buffer = config.buffer;
    voice.key = config.key;
    voice.start = config.start;
    voice.cursor = config.cursor;
    voice.end = config.end;
    voice.gain = config.gain;
    voice.loop = config.loop;
    voice.fadeGain = 0.0f;
    voice.fadeStep = 1.0f / static_cast<float>(fadeFrames(sampleRate));
    voice.lastSample[0] = 0.0f;
    voice.lastSample[1] = 0.0f;
}

void PreviewEngine::beginVoiceFadeOut(PreviewVoiceRuntime& voice,
                                      const double sampleRate) noexcept {
    if (voice.state == VoiceState::inactive || voice.state == VoiceState::fadingOut) return;
    voice.state = VoiceState::fadingOut;
    voice.fadeStep = voice.fadeGain / static_cast<float>(fadeFrames(sampleRate));
    if (voice.fadeGain <= 0.0f) voice.state = VoiceState::inactive;
}

void PreviewEngine::finishVoiceFade(PreviewVoiceRuntime& voice, const std::size_t index,
                                    const double sampleRate) noexcept {
    if (voice.pendingState != nullptr) {
        auto* nextState = voice.pendingState;
        const auto& next = nextState->voices[index];
        voice.pendingState = nullptr;
        audioVoicePendingStates[index].store(nullptr, std::memory_order_release);
        if (next.active) {
            configureVoice(voice, nextState, next, sampleRate);
            audioVoiceStates[index].store(nextState, std::memory_order_release);
            return;
        }
    }
    voice.state = VoiceState::inactive;
    voice.sourceState = nullptr;
    voice.buffer = nullptr;
    audioVoiceStates[index].store(nullptr, std::memory_order_release);
    audioVoicePendingStates[index].store(nullptr, std::memory_order_release);
}

void PreviewEngine::applyPreviewState(PreviewState& state, const double sampleRate) noexcept {
    auto* applied = audioPreviewState.load(std::memory_order_relaxed);
    if (applied != &state) {
        audioPreviewState.store(&state, std::memory_order_release);
        for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
            auto& voice = previewVoices[index];
            const auto& config = state.voices[index];
            if (!config.active) {
                voice.pendingState = nullptr;
                audioVoicePendingStates[index].store(nullptr, std::memory_order_release);
                beginVoiceFadeOut(voice, sampleRate);
                continue;
            }
            const auto sameVoice = voice.state != VoiceState::inactive &&
                                   voice.sourceState != nullptr &&
                                   voice.sourceState->voices[index].revision == config.revision;
            if (voice.state == VoiceState::inactive)
                configureVoice(voice, &state, config, sampleRate);
            else if (!sameVoice) {
                voice.pendingState = &state;
                audioVoicePendingStates[index].store(&state, std::memory_order_release);
                beginVoiceFadeOut(voice, sampleRate);
            }
            if (voice.state == VoiceState::inactive) finishVoiceFade(voice, index, sampleRate);
            if (voice.state != VoiceState::inactive)
                audioVoiceStates[index].store(voice.sourceState, std::memory_order_release);
        }
        const auto desired = state.builtInSession;
        const auto desiredBuffer = state.builtInBuffer;
        const auto current = audioBuiltInSession.load(std::memory_order_acquire);
        if (desired != current) {
            if (current != nullptr) {
                current->allNotesOff();
                pendingBuiltInSession = desired;
                audioBuiltInPendingSession.store(desired, std::memory_order_release);
                audioBuiltInPendingBuffer.store(desiredBuffer, std::memory_order_release);
                builtInFadeAnchor[0] = builtInLastOutput[0];
                builtInFadeAnchor[1] = builtInLastOutput[1];
                builtInState = BuiltInState::fadingOut;
                builtInFadeStep = builtInGain / static_cast<float>(fadeFrames(sampleRate));
            } else if (desired != nullptr) {
                audioBuiltInSession.store(desired, std::memory_order_release);
                audioBuiltInPendingSession.store(nullptr, std::memory_order_release);
                audioBuiltInBuffer.store(desiredBuffer, std::memory_order_release);
                audioBuiltInPendingBuffer.store(nullptr, std::memory_order_release);
                builtInGain = 0.0f;
                builtInFadeStep = 1.0f / static_cast<float>(fadeFrames(sampleRate));
                builtInState = BuiltInState::fadingIn;
            }
        }
    }
    if (builtInStopRequested.exchange(false, std::memory_order_acq_rel)) {
        if (auto* session = audioBuiltInSession.load(std::memory_order_acquire); session != nullptr)
            session->allNotesOff();
        if (audioBuiltInSession.load(std::memory_order_acquire) != nullptr) {
            builtInFadeAnchor[0] = builtInLastOutput[0];
            builtInFadeAnchor[1] = builtInLastOutput[1];
            builtInState = BuiltInState::fadingOut;
            builtInFadeStep = builtInGain / static_cast<float>(fadeFrames(sampleRate));
        }
    }
}

void PreviewEngine::mixPreview(float* const* outputChannelData, const int numOutputChannels,
                               const int numSamples, const double sampleRate) noexcept {
    for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
        auto& voice = previewVoices[index];
        if (voice.state == VoiceState::inactive || voice.buffer == nullptr) continue;
        const auto sourceChannels = voice.buffer->getNumChannels();
        for (int sample = 0; sample < numSamples && voice.state != VoiceState::inactive; ++sample) {
            if (voice.cursor >= voice.end && voice.state != VoiceState::fadingOut) {
                if (voice.loop)
                    voice.cursor = voice.start;
                else
                    beginVoiceFadeOut(voice, sampleRate);
            }
            const auto sourceAvailable = voice.cursor >= voice.start && voice.cursor < voice.end;
            for (int channel = 0; channel < numOutputChannels; ++channel) {
                auto* output = outputChannelData[channel];
                if (output == nullptr) continue;
                const auto sourceChannel = juce::jmin(channel, sourceChannels - 1);
                const auto source = sourceAvailable
                                        ? voice.buffer->getSample(sourceChannel, voice.cursor)
                                        : voice.lastSample[juce::jmin(channel, 1)];
                voice.lastSample[juce::jmin(channel, 1)] = source;
                output[sample] += source * voice.gain * voice.fadeGain;
            }
            if (sourceAvailable) ++voice.cursor;
            if (voice.state == VoiceState::fadingIn) {
                voice.fadeGain = std::min(1.0f, voice.fadeGain + voice.fadeStep);
                if (voice.fadeGain >= 1.0f) voice.state = VoiceState::playing;
            } else if (voice.state == VoiceState::fadingOut) {
                voice.fadeGain = std::max(0.0f, voice.fadeGain - voice.fadeStep);
                if (voice.fadeGain <= 0.0f) finishVoiceFade(voice, index, sampleRate);
            }
            audioVoiceCursors[index].store(voice.cursor, std::memory_order_release);
        }
    }
}

void PreviewEngine::mixBuiltInPreview(float* const* outputChannelData, const int numOutputChannels,
                                      const int numSamples, const double sampleRate) noexcept {
    auto* session = audioBuiltInSession.load(std::memory_order_acquire);
    auto* buffer = audioBuiltInBuffer.load(std::memory_order_acquire);
    if (session == nullptr || buffer == nullptr || numSamples <= 0 ||
        numSamples > buffer->getNumSamples())
        return;
    buffer->clear(0, numSamples);
    session->process(buffer->getArrayOfWritePointers(), buffer->getNumChannels(), numSamples,
                     sampleRate);
    const auto naturalFinish = session->isFinished() && builtInState != BuiltInState::fadingOut;
    const auto fadingOut = builtInState == BuiltInState::fadingOut;
    const auto direction = builtInState == BuiltInState::fadingIn    ? builtInFadeStep
                           : builtInState == BuiltInState::fadingOut ? -builtInFadeStep
                                                                     : 0.0f;
    const auto endpointFrames = std::min(numSamples, fadeFrames(sampleRate));
    for (int sample = 0; sample < numSamples; ++sample) {
        auto gain = juce::jlimit(0.0f, 1.0f, builtInGain + direction * sample);
        if (naturalFinish && sample >= numSamples - endpointFrames) {
            const auto progress = static_cast<float>(sample - (numSamples - endpointFrames)) /
                                  static_cast<float>(std::max(1, endpointFrames));
            gain *= 1.0f - progress;
        }
        for (int channel = 0; channel < numOutputChannels; ++channel) {
            auto* output = outputChannelData[channel];
            if (output == nullptr) continue;
            const auto sourceChannel = juce::jmin(channel, 1);
            auto source = buffer->getSample(sourceChannel, sample);
            if (fadingOut) {
                const auto progress = juce::jlimit(
                    0.0f, 1.0f,
                    static_cast<float>(sample) / static_cast<float>(fadeFrames(sampleRate)));
                source = builtInFadeAnchor[sourceChannel] * (1.0f - progress) + source * progress;
            }
            output[sample] += source * gain;
            builtInLastOutput[sourceChannel] = source * gain;
        }
    }
    builtInGain = juce::jlimit(0.0f, 1.0f, builtInGain + direction * numSamples);
    if (naturalFinish || (builtInState == BuiltInState::fadingOut && builtInGain <= 0.0f)) {
        audioBuiltInSession.store(nullptr, std::memory_order_release);
        audioBuiltInBuffer.store(nullptr, std::memory_order_release);
        builtInState = BuiltInState::inactive;
        builtInGain = 0.0f;
        const auto* next = audioBuiltInPendingSession.exchange(nullptr, std::memory_order_acq_rel);
        auto* nextBuffer = audioBuiltInPendingBuffer.exchange(nullptr, std::memory_order_acq_rel);
        pendingBuiltInSession = nullptr;
        if (next != nullptr) {
            audioBuiltInSession.store(const_cast<InstrumentPreviewSession*>(next),
                                      std::memory_order_release);
            audioBuiltInBuffer.store(nextBuffer, std::memory_order_release);
            builtInState = BuiltInState::fadingIn;
            builtInFadeStep = 1.0f / static_cast<float>(fadeFrames(sampleRate));
        }
    } else if (builtInState == BuiltInState::fadingIn && builtInGain >= 1.0f) {
        builtInGain = 1.0f;
        builtInState = BuiltInState::playing;
    }
}

void PreviewEngine::syncSynthVoices() noexcept {
    if (synthPanicRequested.exchange(false, std::memory_order_acq_rel))
        for (auto& control : synthControl) control.releasing.store(true, std::memory_order_release);
    for (std::size_t index = 0; index < kSynthVoiceCount; ++index) {
        auto& voice = synthVoices[index];
        auto& control = synthControl[index];
        const auto active = control.active.load(std::memory_order_acquire);
        const auto revision = control.revision.load(std::memory_order_acquire);
        if (active && revision != voice.controlRevision) {
            voice.note = control.note.load(std::memory_order_relaxed);
            voice.frequency = control.frequency.load(std::memory_order_relaxed);
            voice.targetLevel = control.targetLevel.load(std::memory_order_relaxed);
            voice.phase = 0.0f;
            voice.level = 0.0f;
            voice.releasing = false;
            voice.active = true;
            voice.controlRevision = revision;
        }
        if (voice.active && (!active || control.releasing.load(std::memory_order_acquire)))
            voice.releasing = true;
    }
}

void PreviewEngine::mixSynth(float* const* outputChannelData, const int numOutputChannels,
                             const int numSamples, const double sampleRate) noexcept {
    syncSynthVoices();
    if (sampleRate <= 0.0 || numOutputChannels <= 0) return;
    constexpr float twoPi = static_cast<float>(kTwoPi);
    for (std::size_t index = 0; index < kSynthVoiceCount; ++index) {
        auto& voice = synthVoices[index];
        auto& control = synthControl[index];
        if (!voice.active) continue;
        const auto phaseStep = static_cast<float>(twoPi * voice.frequency / sampleRate);
        for (int sample = 0; sample < numSamples && voice.active; ++sample) {
            if (voice.releasing) {
                voice.level *= 0.995f;
                if (voice.level < 0.0001f) {
                    voice.active = false;
                    const auto releaseRevision = voice.controlRevision;
                    if (control.revision.load(std::memory_order_acquire) == releaseRevision)
                        control.audioFinishedRevision.store(releaseRevision,
                                                            std::memory_order_release);
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

bool PreviewEngine::tryMix(float* const* outputChannelData, const int numOutputChannels,
                           const int numSamples, const double sampleRate) noexcept {
    const PreviewAudioGuard previewRead(*this);
    auto* state = pendingPreviewState.load(std::memory_order_acquire);
    if (state != nullptr) applyPreviewState(*state, sampleRate);
    mixBuiltInPreview(outputChannelData, numOutputChannels, numSamples, sampleRate);
    mixPreview(outputChannelData, numOutputChannels, numSamples, sampleRate);
    mixSynth(outputChannelData, numOutputChannels, numSamples, sampleRate);
    return true;
}

void PreviewEngine::requestSynthPanic() noexcept {
    synthPanicRequested.store(true, std::memory_order_release);
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
