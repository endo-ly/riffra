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
    for (auto& cursor : audioVoiceCursors) cursor.store(0, std::memory_order_relaxed);
}

PreviewEngine::~PreviewEngine() = default;

bool PreviewEngine::publishState(std::unique_ptr<PreviewState> next,
                                 const PreviewCommand::Kind kind, juce::String& error,
                                 const std::size_t index, const int key) {
    if (previewStates.size() == audioStates.size()) {
        error = "The realtime preview retirement queue is full.";
        reclaimRetiredState();
        return false;
    }
    PreviewCommand command;
    command.kind = kind;
    command.state = next.get();
    command.index = index;
    command.key = key;
    command.session = next->instrumentSession;
    command.buffer = next->instrumentBuffer;
    previewStates.push_back(std::move(next));
    if (!commands.tryPush(command)) {
        previewStates.pop_back();
        error = "The realtime preview command queue is full.";
        reclaimRetiredState();
        return false;
    }
    controlState = *command.state;
    return true;
}

void PreviewEngine::reclaimRetiredState() {
    retiredStates.reclaim([this](PreviewState* retired) {
        std::erase_if(previewStates,
                      [retired](const auto& state) { return state.get() == retired; });
    });
    const auto stateUsesBuffer = [](const PreviewState& state, const auto* buffer) {
        return std::any_of(state.voices.begin(), state.voices.end(),
                           [buffer](const auto& voice) { return voice.buffer == buffer; });
    };
    std::erase_if(previewBuffers, [&](const auto& buffer) {
        if (stateUsesBuffer(controlState, buffer.get())) return false;
        return std::none_of(previewStates.begin(), previewStates.end(), [&](const auto& state) {
            return stateUsesBuffer(*state, buffer.get());
        });
    });
    std::erase_if(instrumentSessions, [&](const auto& session) {
        if (controlState.instrumentSession == session.get()) return false;
        return std::none_of(previewStates.begin(), previewStates.end(), [&](const auto& state) {
            return state->instrumentSession == session.get();
        });
    });
    std::erase_if(instrumentBuffers, [&](const auto& buffer) {
        if (controlState.instrumentBuffer == buffer.get()) return false;
        return std::none_of(previewStates.begin(), previewStates.end(), [&](const auto& state) {
            return state->instrumentBuffer == buffer.get();
        });
    });
}

void PreviewEngine::retireUnusedStates() noexcept {
    for (std::size_t index = 0; index < audioStateCount;) {
        auto* state = audioStates[index];
        const auto needed =
            state == audioPreviewState ||
            std::any_of(previewVoices.begin(), previewVoices.end(),
                        [state](const auto& voice) {
                            return voice.sourceState == state || voice.pendingState == state;
                        }) ||
            (state->instrumentSession != nullptr &&
             (state->instrumentSession == audioInstrumentSession ||
              state->instrumentSession == audioInstrumentPendingSession));
        if (needed) {
            ++index;
            continue;
        }
        if (!retiredStates.retire(state)) {
            jassertfalse;
            return;
        }
        audioStates[index] = audioStates[--audioStateCount];
    }
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

    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
    if (voiceKey < 0) {
        next->instrumentSession = nullptr;
        next->instrumentBuffer = nullptr;
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
    return publishState(std::move(next),
                        voiceKey < 0 ? PreviewCommand::Kind::publishPreviewState
                                     : PreviewCommand::Kind::publishVoice,
                        error, targetIndex);
}

bool PreviewEngine::startInstrumentPreview(const juce::String& definitionJson,
                                           const juce::String& definitionBaseDir,
                                           InstrumentPreviewSpec spec, const double sampleRate,
                                           const int blockSize, juce::String& error) {
    auto session = InstrumentPreviewSession::create(definitionJson, definitionBaseDir,
                                                    std::move(spec), sampleRate, blockSize, error);
    if (session == nullptr) return false;

    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
    for (auto& voice : next->voices) {
        if (!voice.active || voice.key == 1) continue;
        voice.active = false;
        voice.buffer = nullptr;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    instrumentSessions.push_back(std::move(session));
    auto instrumentBuffer = std::make_unique<juce::AudioBuffer<float>>();
    instrumentBuffer->setSize(2, std::max(1, blockSize), false, true, true);
    auto* instrumentBufferPtr = instrumentBuffer.get();
    instrumentBuffers.push_back(std::move(instrumentBuffer));
    next->instrumentSession = instrumentSessions.back().get();
    next->instrumentBuffer = instrumentBufferPtr;
    return publishState(std::move(next), PreviewCommand::Kind::startInstrumentPreview, error);
}

bool PreviewEngine::stopInstrumentPreview(juce::String* error) {
    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
    next->instrumentSession = nullptr;
    next->instrumentBuffer = nullptr;
    juce::String failure;
    const auto accepted =
        publishState(std::move(next), PreviewCommand::Kind::stopInstrumentPreview, failure);
    if (error != nullptr) *error = failure;
    return accepted;
}

bool PreviewEngine::stopPreview(juce::String* error) {
    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
    next->instrumentSession = nullptr;
    next->instrumentBuffer = nullptr;
    for (auto& voice : next->voices) {
        voice.active = false;
        voice.buffer = nullptr;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    juce::String failure;
    const auto accepted = publishState(std::move(next), PreviewCommand::Kind::stopAll, failure);
    if (error != nullptr) *error = failure;
    return accepted;
}

bool PreviewEngine::stopPreviewForKey(const int voiceKey, juce::String* error) {
    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
    for (auto& voice : next->voices) {
        if (!voice.active || voice.key != voiceKey) continue;
        voice.active = false;
        voice.buffer = nullptr;
        voice.key = -1;
        voice.cursor = voice.start;
        voice.loop = false;
        voice.revision = ++previewSequence;
    }
    juce::String failure;
    const auto accepted =
        publishState(std::move(next), PreviewCommand::Kind::stopVoice, failure, 0, voiceKey);
    if (error != nullptr) *error = failure;
    return accepted;
}

bool PreviewEngine::switchPreviewBuffer(const int voiceKey, const juce::AudioBuffer<float>& buffer,
                                        juce::String& error) {
    if (buffer.getNumChannels() <= 0 || buffer.getNumSamples() <= 0) {
        error = "Take comparison source contains no audio.";
        return false;
    }
    const std::lock_guard lock(controlMutex);
    reclaimRetiredState();
    auto next = std::make_unique<PreviewState>(controlState);
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
        return publishState(std::move(next), PreviewCommand::Kind::publishVoice, error, index);
    }
    error = "Take comparison is not active.";
    return false;
}

bool PreviewEngine::startSynthNote(const int note, const float velocity) noexcept {
    if (note < 0 || note > 127) return false;
    PreviewCommand command;
    command.kind = PreviewCommand::Kind::synthNoteOn;
    command.note = note;
    command.velocity = velocity;
    return commands.tryPush(command);
}

bool PreviewEngine::stopSynthNote(const int note) noexcept {
    PreviewCommand command;
    command.kind = PreviewCommand::Kind::synthNoteOff;
    command.note = note;
    return commands.tryPush(command);
}

bool PreviewEngine::allNotesOff() noexcept { return requestSynthPanic(); }

bool PreviewEngine::isPreviewing() const noexcept {
    return previewing.load(std::memory_order_acquire);
}

bool PreviewEngine::isInstrumentPreviewing() const noexcept {
    return instrumentPreviewing.load(std::memory_order_acquire);
}

void PreviewEngine::applyCommand(const PreviewCommand& command, const double sampleRate) noexcept {
    if (command.state != nullptr) {
        jassert(audioStateCount < audioStates.size());
        audioStates[audioStateCount++] = command.state;
        applyPreviewState(*command.state, sampleRate, &command);
        retireUnusedStates();
        return;
    }
    if (command.kind == PreviewCommand::Kind::synthNoteOn) {
        auto* voice = &synthVoices[0];
        const auto same =
            std::find_if(synthVoices.begin(), synthVoices.end(), [&](const auto& candidate) {
                return candidate.active && candidate.note == command.note;
            });
        const auto idle = std::find_if(synthVoices.begin(), synthVoices.end(),
                                       [](const auto& candidate) { return !candidate.active; });
        if (same != synthVoices.end())
            voice = &*same;
        else if (idle != synthVoices.end())
            voice = &*idle;
        *voice = {};
        voice->note = command.note;
        voice->frequency =
            440.0f * std::pow(2.0f, (static_cast<float>(command.note) - 69.0f) / 12.0f);
        voice->targetLevel = juce::jlimit(0.02f, 0.18f, command.velocity) * 0.8f;
        voice->active = true;
    } else if (command.kind == PreviewCommand::Kind::synthNoteOff) {
        for (auto& voice : synthVoices)
            if (voice.active && voice.note == command.note) voice.releasing = true;
    } else if (command.kind == PreviewCommand::Kind::synthPanic) {
        for (auto& voice : synthVoices) voice.releasing = true;
        instrumentStopRequested = true;
        if (audioPreviewState != nullptr) applyPreviewState(*audioPreviewState, sampleRate);
    }
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
        if (next.active) {
            configureVoice(voice, nextState, next, sampleRate);
            return;
        }
    }
    voice.state = VoiceState::inactive;
    voice.sourceState = nullptr;
    voice.buffer = nullptr;
}

void PreviewEngine::applyPreviewState(PreviewState& state, const double sampleRate,
                                      const PreviewCommand* command) noexcept {
    auto* applied = audioPreviewState;
    if (applied != &state) {
        audioPreviewState = &state;
        for (std::size_t index = 0; index < kPreviewVoiceCount; ++index) {
            auto& voice = previewVoices[index];
            if (command != nullptr && command->kind == PreviewCommand::Kind::publishVoice &&
                index != command->index)
                continue;
            if (command != nullptr && command->kind == PreviewCommand::Kind::stopVoice &&
                voice.key != command->key &&
                (voice.pendingState == nullptr ||
                 voice.pendingState->voices[index].key != command->key))
                continue;
            const auto& config = state.voices[index];
            if (!config.active) {
                voice.pendingState = nullptr;
                beginVoiceFadeOut(voice, sampleRate);
                if (voice.state == VoiceState::inactive) finishVoiceFade(voice, index, sampleRate);
                continue;
            }
            const auto sameVoice = voice.state != VoiceState::inactive &&
                                   voice.sourceState != nullptr &&
                                   voice.sourceState->voices[index].revision == config.revision;
            if (voice.state == VoiceState::inactive)
                configureVoice(voice, &state, config, sampleRate);
            else if (!sameVoice) {
                voice.pendingState = &state;
                beginVoiceFadeOut(voice, sampleRate);
            }
            if (voice.state == VoiceState::inactive) finishVoiceFade(voice, index, sampleRate);
        }
        const auto startingInstrument =
            command != nullptr && command->kind == PreviewCommand::Kind::startInstrumentPreview;
        const auto desired = startingInstrument ? command->session : state.instrumentSession;
        const auto desiredBuffer = startingInstrument ? command->buffer : state.instrumentBuffer;
        const auto current = audioInstrumentSession;
        if (desired != current) {
            if (current != nullptr) {
                current->allNotesOff();
                audioInstrumentPendingSession = desired;
                audioInstrumentPendingBuffer = desiredBuffer;
                instrumentFadeAnchor[0] = instrumentLastOutput[0];
                instrumentFadeAnchor[1] = instrumentLastOutput[1];
                instrumentState = InstrumentState::fadingOut;
                instrumentFadeStep = instrumentGain / static_cast<float>(fadeFrames(sampleRate));
            } else if (desired != nullptr) {
                audioInstrumentSession = desired;
                audioInstrumentPendingSession = nullptr;
                audioInstrumentBuffer = desiredBuffer;
                audioInstrumentPendingBuffer = nullptr;
                instrumentGain = 0.0f;
                instrumentFadeStep = 1.0f / static_cast<float>(fadeFrames(sampleRate));
                instrumentState = InstrumentState::fadingIn;
            }
        }
    }
    if (std::exchange(instrumentStopRequested, false)) {
        if (auto* session = audioInstrumentSession; session != nullptr) session->allNotesOff();
        if (audioInstrumentSession != nullptr) {
            instrumentFadeAnchor[0] = instrumentLastOutput[0];
            instrumentFadeAnchor[1] = instrumentLastOutput[1];
            instrumentState = InstrumentState::fadingOut;
            instrumentFadeStep = instrumentGain / static_cast<float>(fadeFrames(sampleRate));
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

void PreviewEngine::mixInstrumentPreview(float* const* outputChannelData,
                                         const int numOutputChannels, const int numSamples,
                                         const double sampleRate) noexcept {
    auto* session = audioInstrumentSession;
    auto* buffer = audioInstrumentBuffer;
    if (session == nullptr || buffer == nullptr || numSamples <= 0 ||
        numSamples > buffer->getNumSamples())
        return;
    buffer->clear(0, numSamples);
    session->process(buffer->getArrayOfWritePointers(), buffer->getNumChannels(), numSamples,
                     sampleRate);
    const auto naturalFinish =
        session->isFinished() && instrumentState != InstrumentState::fadingOut;
    const auto fadingOut = instrumentState == InstrumentState::fadingOut;
    const auto direction = instrumentState == InstrumentState::fadingIn    ? instrumentFadeStep
                           : instrumentState == InstrumentState::fadingOut ? -instrumentFadeStep
                                                                           : 0.0f;
    const auto endpointFrames = std::min(numSamples, fadeFrames(sampleRate));
    for (int sample = 0; sample < numSamples; ++sample) {
        auto gain = juce::jlimit(0.0f, 1.0f, instrumentGain + direction * sample);
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
                source =
                    instrumentFadeAnchor[sourceChannel] * (1.0f - progress) + source * progress;
            }
            output[sample] += source * gain;
            instrumentLastOutput[sourceChannel] = source * gain;
        }
    }
    instrumentGain = juce::jlimit(0.0f, 1.0f, instrumentGain + direction * numSamples);
    if (naturalFinish ||
        (instrumentState == InstrumentState::fadingOut && instrumentGain <= 0.0f)) {
        audioInstrumentSession = nullptr;
        audioInstrumentBuffer = nullptr;
        instrumentState = InstrumentState::inactive;
        instrumentGain = 0.0f;
        const auto* next = std::exchange(audioInstrumentPendingSession, nullptr);
        auto* nextBuffer = std::exchange(audioInstrumentPendingBuffer, nullptr);
        if (next != nullptr) {
            audioInstrumentSession = const_cast<InstrumentPreviewSession*>(next);
            audioInstrumentBuffer = nextBuffer;
            instrumentState = InstrumentState::fadingIn;
            instrumentFadeStep = 1.0f / static_cast<float>(fadeFrames(sampleRate));
        }
    } else if (instrumentState == InstrumentState::fadingIn && instrumentGain >= 1.0f) {
        instrumentGain = 1.0f;
        instrumentState = InstrumentState::playing;
    }
}

void PreviewEngine::mixSynth(float* const* outputChannelData, const int numOutputChannels,
                             const int numSamples, const double sampleRate) noexcept {
    if (sampleRate <= 0.0 || numOutputChannels <= 0) return;
    constexpr float twoPi = static_cast<float>(kTwoPi);
    for (std::size_t index = 0; index < kSynthVoiceCount; ++index) {
        auto& voice = synthVoices[index];
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

bool PreviewEngine::tryMix(float* const* outputChannelData, const int numOutputChannels,
                           const int numSamples, const double sampleRate) noexcept {
    commands.drain(
        [this, sampleRate](const PreviewCommand& command) { applyCommand(command, sampleRate); });
    mixInstrumentPreview(outputChannelData, numOutputChannels, numSamples, sampleRate);
    mixPreview(outputChannelData, numOutputChannels, numSamples, sampleRate);
    mixSynth(outputChannelData, numOutputChannels, numSamples, sampleRate);
    retireUnusedStates();
    instrumentPreviewing.store(
        audioInstrumentSession != nullptr && instrumentState != InstrumentState::fadingOut,
        std::memory_order_release);
    const auto hasVoice =
        std::any_of(previewVoices.begin(), previewVoices.end(),
                    [](const auto& voice) { return voice.state != VoiceState::inactive; });
    const auto hasSynth = std::any_of(synthVoices.begin(), synthVoices.end(),
                                      [](const auto& voice) { return voice.active; });
    previewing.store(hasVoice || hasSynth || audioInstrumentSession != nullptr,
                     std::memory_order_release);
    return true;
}

bool PreviewEngine::requestSynthPanic() noexcept {
    PreviewCommand command;
    command.kind = PreviewCommand::Kind::synthPanic;
    return commands.tryPush(command);
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
