#include "TimelineEngine.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

namespace riffra {

void TimelineEngine::mixMetronome(float* const* outputChannels, const int channelCount,
                                  const int sampleCount) noexcept {
    if (sampleCount <= 0) return;
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    if (active == nullptr || !active->metronomeEnabled || active->beatSamples <= 0) return;
    const auto loopLength = active->loopEndSample - active->loopStartSample;
    const auto start = lastMixStartSample.load(std::memory_order_acquire);
    const auto playbackOffset =
        juce::jlimit(0, sampleCount, lastMixPlaybackOffset.load(std::memory_order_acquire));
    const auto countInRemaining = countInBlockStartRemainingSamples.load(std::memory_order_acquire);
    const auto countingIn =
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::countingIn;
    const auto playing = state.load(std::memory_order_acquire) == State::playing;
    constexpr std::int64_t clickSamples = 1'920;
    for (int sample = 0; sample < sampleCount; ++sample) {
        float value = 0.0f;
        if (countInRemaining > 0 && sample < (countingIn ? sampleCount : playbackOffset)) {
            const auto remaining = countInRemaining - sample;
            const auto offset =
                (active->beatSamples - remaining % active->beatSamples) % active->beatSamples;
            if (offset >= 0 && offset < clickSamples) {
                const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                value = 0.11f * envelope;
            }
        } else if (playing && sample >= playbackOffset) {
            auto position = start + sample - playbackOffset;
            if (active->loopEnabled && loopLength > 0 && position >= active->loopEndSample)
                position =
                    active->loopStartSample + (position - active->loopEndSample) % loopLength;
            if (position >= 0) {
                const auto beat = position / active->beatSamples;
                const auto offset = position % active->beatSamples;
                if (offset >= 0 && offset < clickSamples) {
                    const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                    const auto amplitude = beat % active->beatsPerBar == 0 ? 0.18f : 0.11f;
                    value = amplitude * envelope;
                }
            }
        }
        if (value <= 0.0f) continue;
        for (int channel = 0; channel < channelCount; ++channel) {
            if (outputChannels[channel] != nullptr) outputChannels[channel][sample] += value;
        }
    }
}

void TimelineEngine::mixRange(Track& track, const std::int64_t rangeStart,
                              const int destinationStart, const int sampleCount) noexcept {
    auto& runtime = *track.runtime;
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : track.clips) {
        auto& clip = *clipPtr;
        if (clip.muted) continue;
        const auto clipEnd = clip.startSample + clip.durationSamples;
        const auto overlapStart = std::max(rangeStart, clip.startSample);
        const auto overlapEnd = std::min(rangeEnd, clipEnd);
        if (overlapEnd <= overlapStart) continue;
        auto& destinationBuffer = clip.processingStage == ProcessingStage::PostEffects
                                      ? runtime.postEffectClipBuffer
                                      : runtime.mixBuffer;
        auto remaining = static_cast<int>(overlapEnd - overlapStart);
        auto outputOffset = destinationStart + static_cast<int>(overlapStart - rangeStart);
        auto localSample = overlapStart - clip.startSample;
        while (remaining > 0) {
            const auto sourceRange = clip.sourceEndFrame - clip.sourceStartFrame;
            auto sourceOffset = static_cast<std::int64_t>(
                std::floor(static_cast<double>(localSample) * clip.sourceSampleRate /
                           runtime.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd =
                static_cast<int>(std::ceil(static_cast<double>(sourceRemaining) *
                                           runtime.outputSampleRate / clip.sourceSampleRate));
            const auto chunk = std::min(remaining, std::max(1, outputUntilSourceEnd));
            if (clip.expectedSourceFrame < 0 ||
                std::abs(clip.expectedSourceFrame - sourceFrame) > 2) {
                clip.positionableSource->setNextReadPosition(sourceFrame);
                clip.resamplingSource->flushBuffers();
            }
            clip.scratch.clear();
            clip.resamplingSource->getNextAudioBlock(
                juce::AudioSourceChannelInfo(&clip.scratch, 0, chunk));
            for (int sample = 0; sample < chunk; ++sample) {
                const auto position = localSample + sample;
                auto envelope = 1.0f;
                if (clip.fadeInSamples > 0 && position < clip.fadeInSamples) {
                    const auto progress =
                        static_cast<float>(position) / static_cast<float>(clip.fadeInSamples);
                    envelope = std::min(envelope, fadeEnvelope(progress, clip.fadeShape));
                }
                const auto remainingClip = clip.durationSamples - position - 1;
                if (clip.fadeOutSamples > 0 && remainingClip < clip.fadeOutSamples) {
                    const auto progress =
                        static_cast<float>(std::max<std::int64_t>(0, remainingClip)) /
                        static_cast<float>(clip.fadeOutSamples);
                    envelope = std::min(envelope, fadeEnvelope(progress, clip.fadeShape));
                }
                const auto source = clip.scratch.getSample(0, sample) * envelope;
                destinationBuffer.addSample(0, outputOffset + sample, source * clip.leftGain);
                destinationBuffer.addSample(
                    1, outputOffset + sample,
                    clip.scratch.getNumChannels() > 1
                        ? clip.scratch.getSample(1, sample) * envelope * clip.rightGain
                        : source * clip.rightGain);
            }
            clip.expectedSourceFrame =
                sourceFrame +
                static_cast<std::int64_t>(std::floor(
                    static_cast<double>(chunk) * clip.sourceSampleRate / runtime.outputSampleRate));
            remaining -= chunk;
            outputOffset += chunk;
            localSample += chunk;
            if (!clip.loop && sourceFrame + sourceRemaining >= clip.sourceEndFrame && remaining > 0)
                break;
            if (clip.loop && remaining > 0) clip.expectedSourceFrame = -1;
        }
    }
}

void TimelineEngine::scheduleMidi(const PreparedTimeline& prepared, Track& track,
                                  const std::int64_t rangeStart, const int sampleCount) noexcept {
    juce::ignoreUnused(prepared);
    auto& runtime = *track.runtime;
    runtime.midiBuffer.clear();
    MidiScheduler::schedule(runtime.midiClips, rangeStart, sampleCount, runtime.midiBuffer);
}

void TimelineEngine::processTracks(PreparedTimeline& prepared,
                                   const float* const* physicalInputChannels,
                                   const int physicalInputChannelCount,
                                   float* const* outputChannels, const int channelCount,
                                   const std::int64_t rangeStart, const int destinationStart,
                                   const int sampleCount) noexcept {
    const auto hasSolo = prepared.hasSolo;
    processLiveAudioTracks(prepared, physicalInputChannels, physicalInputChannelCount,
                           outputChannels, channelCount, rangeStart, destinationStart, sampleCount);
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        runtime.processedBuffer.clear(0, sampleCount);
        if (!runtime.instrumentTrack) mergeTimelineAndLiveInput(track, sampleCount);
        const float* inputChannels[2] = {runtime.mixBuffer.getWritePointer(0),
                                         runtime.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {runtime.processedBuffer.getWritePointer(0),
                                       runtime.processedBuffer.getWritePointer(1)};
        if (runtime.instrumentTrack)
            processInstrumentTrack(prepared, track, sampleCount, &runtime.midiBuffer, rangeStart);
        else
            runtime.effects().process(inputChannels, 2, processedChannels, 2, sampleCount);
        mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart, destinationStart,
                       sampleCount);
    }
}

void TimelineEngine::processLiveAudioTracks(PreparedTimeline& prepared,
                                            const float* const* physicalInputChannels,
                                            const int physicalInputChannelCount,
                                            float* const* outputChannels, const int channelCount,
                                            const std::int64_t rangeStart,
                                            const int destinationStart, const int sampleCount,
                                            const bool renderOutput) noexcept {
    const auto hasSolo = prepared.hasSolo;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        if (runtime.instrumentTrack) continue;
        runtime.liveInputBuffer.clear(0, sampleCount);
        if ((runtime.monitorInput || runtime.armed) && runtime.audioInputChannel >= 0) {
            const auto* source = ArrangementGraph::audioInputSource(
                runtime.audioInputChannel, physicalInputChannels, physicalInputChannelCount);
            for (int channel = 0; channel < 2; ++channel) {
                auto* destination = runtime.liveInputBuffer.getWritePointer(channel);
                if (source != nullptr)
                    juce::FloatVectorOperations::copy(destination, source + destinationStart,
                                                      sampleCount);
                else
                    juce::FloatVectorOperations::clear(destination, sampleCount);
            }
            if (renderOutput && runtime.monitorInput) {
                runtime.mixBuffer.clear(0, sampleCount);
                for (int channel = 0; channel < 2; ++channel) {
                    juce::FloatVectorOperations::add(
                        runtime.mixBuffer.getWritePointer(channel),
                        runtime.liveInputBuffer.getReadPointer(channel), sampleCount);
                }
            }
            const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
            const auto captureEnd =
                captureStart + captureBlockSamples.load(std::memory_order_acquire);
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (runtime.armed) {
                auto& capture = runtime.recordingCapture;
                if (writeEnd > writeStart) {
                    const auto localOffset = writeStart - destinationStart;
                    const auto captureAudioStart =
                        callbackAudioStartSample.load(std::memory_order_acquire) +
                        static_cast<std::uint64_t>(writeStart);
                    const auto captureTimelineStart =
                        static_cast<std::uint64_t>(rangeStart + localOffset);
                    const auto discontinuous = capture.state != RecordingCaptureState::capturing ||
                                               captureAudioStart != capture.endAudioSample;
                    if (discontinuous && capture.state == RecordingCaptureState::capturing) {
                        if (!recordingCapture->endTrackCapture(track.id, capture))
                            capture.state = RecordingCaptureState::idle;
                        else
                            capture.state = RecordingCaptureState::idle;
                    }
                    if (capture.state == RecordingCaptureState::idle) {
                        (void)recordingCapture->beginTrackCapture(
                            track.id, capture, captureAudioStart, captureTimelineStart);
                    }
                    if (capture.state == RecordingCaptureState::capturing) {
                        const auto writeCount = writeEnd - writeStart;
                        const auto* rawPointer =
                            runtime.liveInputBuffer.getReadPointer(0) + localOffset;
                        recordingCapture->writeAudioTrack(track.id, rawPointer, writeCount);
                        capture.endAudioSample =
                            captureAudioStart + static_cast<std::uint64_t>(writeCount);
                        capture.endTimelineSample =
                            captureTimelineStart + static_cast<std::uint64_t>(writeCount);
                    }
                } else if (capture.state == RecordingCaptureState::capturing) {
                    if (!recordingCapture->endTrackCapture(track.id, capture))
                        capture.state = RecordingCaptureState::idle;
                    else
                        capture.state = RecordingCaptureState::idle;
                }
            }
            if (renderOutput && runtime.monitorInput) {
                runtime.processedBuffer.clear(0, sampleCount);
                runtime.effects().process(runtime.mixBuffer.getArrayOfReadPointers(), 2,
                                          runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                          sampleCount);
                mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart,
                               destinationStart, sampleCount);
            }
        }
    }
}

void TimelineEngine::mergeTimelineAndLiveInput(Track& track, const int sampleCount) noexcept {
    auto& runtime = *track.runtime;
    if (sampleCount <= 0) return;
    if (!runtime.monitorInput) return;
    for (int channel = 0; channel < 2; ++channel)
        juce::FloatVectorOperations::add(runtime.mixBuffer.getWritePointer(channel),
                                         runtime.liveInputBuffer.getReadPointer(channel),
                                         sampleCount);
}

void TimelineEngine::processLiveInstrumentTracks(PreparedTimeline& prepared,
                                                 float* const* outputChannels,
                                                 const int channelCount,
                                                 const std::int64_t rangeStart,
                                                 const int sampleCount) noexcept {
    const auto hasSolo = prepared.hasSolo;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        if (!runtime.instrumentTrack) continue;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        processLiveInstrumentTrack(prepared, track, sampleCount, rangeStart, false);
        mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart, 0, sampleCount);
    }
}

void TimelineEngine::processInstrumentTrack(PreparedTimeline& prepared, Track& track,
                                            const int sampleCount,
                                            const juce::MidiBuffer* const timelineMidi,
                                            const std::int64_t rangeStart) noexcept {
    auto& runtime = *track.runtime;
    if (runtime.instrument() != nullptr) {
        runtime.instrument()->process(runtime.mixBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                      timelineMidi,
                                      instrumentProcessContext(prepared, rangeStart, true));
    } else {
        runtime.mixBuffer.clear(0, sampleCount);
    }
    runtime.effects().process(runtime.mixBuffer.getArrayOfReadPointers(), 2,
                              runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount);
}

void TimelineEngine::processLiveInstrumentTrack(PreparedTimeline& prepared, Track& track,
                                                const int sampleCount,
                                                const std::int64_t rangeStart,
                                                const bool playing) noexcept {
    auto& runtime = *track.runtime;
    runtime.liveInputBuffer.clear(0, sampleCount);
    if (runtime.instrument() == nullptr || !runtime.liveMidiActive()) {
        runtime.processedBuffer.clear(0, sampleCount);
        return;
    }
    runtime.instrument()->process(runtime.liveInputBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                  nullptr, instrumentProcessContext(prepared, rangeStart, playing));
    runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                              runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount);
    runtime.markLiveMidiProcessed(sampleCount);
}

void TimelineEngine::mixTrackOutput(Track& track, const bool audible, float* const* outputChannels,
                                    const int channelCount, const std::int64_t rangeStart,
                                    const int destinationStart, const int sampleCount) noexcept {
    auto& runtime = *track.runtime;
    const auto lowLatency = runtime.lowLatencyMonitoring();
    const auto processedDelay =
        lowLatency ? std::int64_t{0} : std::max<std::int64_t>(0, runtime.compensationDelaySamples);
    const auto processedDelaySize = runtime.delayBuffer.getNumSamples();
    const auto postEffectDelay =
        lowLatency ? std::int64_t{0}
                   : std::max<std::int64_t>(0, runtime.postEffectCompensationDelaySamples);
    const auto postEffectDelaySize = runtime.postEffectDelayBuffer.getNumSamples();
    auto volumeCursor = runtime.volumeAutomation.cursorAt(rangeStart);
    auto panCursor = runtime.panAutomation.cursorAt(rangeStart);
    const auto volumeAutomated = !runtime.volumeAutomation.empty();
    const auto panAutomated = !runtime.panAutomation.empty();
    const auto fixedPanAngle = (runtime.pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
    const auto fixedGain = juce::Decibels::decibelsToGain(runtime.gainDb);
    const auto fixedLeftGain = fixedGain * std::cos(fixedPanAngle);
    const auto fixedRightGain = fixedGain * std::sin(fixedPanAngle);
    const auto blockEnd = rangeStart + sampleCount;
    int processed = 0;
    while (processed < sampleCount) {
        const auto absoluteSample = rangeStart + processed;
        const auto volumeSegment =
            volumeAutomated ? volumeCursor.segmentAt(absoluteSample, blockEnd, runtime.gainDb)
                            : AutomationRuntime::Segment{blockEnd, runtime.gainDb, runtime.gainDb};
        const auto panSegment =
            panAutomated ? panCursor.segmentAt(absoluteSample, blockEnd, runtime.pan)
                         : AutomationRuntime::Segment{blockEnd, runtime.pan, runtime.pan};
        const auto segmentEnd = std::min({blockEnd, volumeSegment.endSample, panSegment.endSample});
        const auto segmentSamples =
            static_cast<int>(std::max<std::int64_t>(1, segmentEnd - absoluteSample));

        auto currentGain = fixedGain;
        auto gainRatio = 1.0f;
        auto currentCos = std::cos(fixedPanAngle);
        auto currentSin = std::sin(fixedPanAngle);
        auto deltaCos = 1.0f;
        auto deltaSin = 0.0f;
        if (volumeAutomated) {
            const auto startGain = juce::Decibels::decibelsToGain(
                juce::jlimit(-90.0f, 24.0f, volumeSegment.startValue));
            const auto endGain =
                juce::Decibels::decibelsToGain(juce::jlimit(-90.0f, 24.0f, volumeSegment.endValue));
            currentGain = startGain;
            gainRatio =
                startGain > 0.0f ? std::pow(endGain / startGain, 1.0f / segmentSamples) : 1.0f;
        }
        if (panAutomated) {
            const auto startAngle = (juce::jlimit(-1.0f, 1.0f, panSegment.startValue) + 1.0f) *
                                    juce::MathConstants<float>::pi * 0.25f;
            const auto endAngle = (juce::jlimit(-1.0f, 1.0f, panSegment.endValue) + 1.0f) *
                                  juce::MathConstants<float>::pi * 0.25f;
            currentCos = std::cos(startAngle);
            currentSin = std::sin(startAngle);
            const auto delta = (endAngle - startAngle) / segmentSamples;
            deltaCos = std::cos(delta);
            deltaSin = std::sin(delta);
        }

        for (int offset = 0; offset < segmentSamples && processed < sampleCount;
             ++offset, ++processed) {
            auto leftGain = fixedLeftGain;
            auto rightGain = fixedRightGain;
            if (volumeAutomated || panAutomated) {
                leftGain = currentGain * currentCos;
                rightGain = currentGain * currentSin;
            }
            float left = runtime.processedBuffer.getSample(0, processed);
            float right = runtime.processedBuffer.getSample(1, processed);
            if (processedDelaySize > 0) {
                const auto write = runtime.delayWritePosition;
                runtime.delayBuffer.setSample(0, static_cast<int>(write), left);
                runtime.delayBuffer.setSample(1, static_cast<int>(write), right);
                if (processedDelay > 0) {
                    const auto read =
                        (write - processedDelay + processedDelaySize) % processedDelaySize;
                    left = runtime.delayBuffer.getSample(0, static_cast<int>(read));
                    right = runtime.delayBuffer.getSample(1, static_cast<int>(read));
                }
                runtime.delayWritePosition = (write + 1) % processedDelaySize;
            }
            auto postEffectLeft = runtime.postEffectClipBuffer.getSample(0, processed);
            auto postEffectRight = runtime.postEffectClipBuffer.getSample(1, processed);
            if (postEffectDelaySize > 0) {
                const auto write = runtime.postEffectDelayWritePosition;
                runtime.postEffectDelayBuffer.setSample(0, static_cast<int>(write), postEffectLeft);
                runtime.postEffectDelayBuffer.setSample(1, static_cast<int>(write),
                                                        postEffectRight);
                if (postEffectDelay > 0) {
                    const auto read =
                        (write - postEffectDelay + postEffectDelaySize) % postEffectDelaySize;
                    postEffectLeft =
                        runtime.postEffectDelayBuffer.getSample(0, static_cast<int>(read));
                    postEffectRight =
                        runtime.postEffectDelayBuffer.getSample(1, static_cast<int>(read));
                }
                runtime.postEffectDelayWritePosition = (write + 1) % postEffectDelaySize;
            }
            left += postEffectLeft;
            right += postEffectRight;
            if (audible && channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + processed] += left * leftGain;
            if (audible && channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + processed] += right * rightGain;
            if (volumeAutomated) currentGain *= gainRatio;
            if (panAutomated) {
                const auto nextCos = currentCos * deltaCos - currentSin * deltaSin;
                currentSin = currentSin * deltaCos + currentCos * deltaSin;
                currentCos = nextCos;
            }
        }
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    clearPlaybackTrackState(prepared);
    for (auto& trackPtr : prepared.tracks) trackPtr->runtime->resetForTransportDiscontinuity();
}

void TimelineEngine::clearPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        runtime.mixBuffer.clear();
        runtime.processedBuffer.clear();
        runtime.postEffectClipBuffer.clear();
        runtime.midiBuffer.clear();
        runtime.delayBuffer.clear();
        runtime.delayWritePosition = 0;
        runtime.postEffectDelayBuffer.clear();
        runtime.postEffectDelayWritePosition = 0;
    }
}

void TimelineEngine::applyPendingPanic(PreparedTimeline& prepared) noexcept {
    if (!panicAllPending.exchange(false, std::memory_order_acq_rel)) return;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.runtime != nullptr) track.runtime->panic();
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.runtime->recordingCapture);
    }
}

void TimelineEngine::requestPlaybackReset() noexcept {
    PreparedTimeline* active = nullptr;
    if (!beginAudioRead(active)) return;
    if (active != nullptr)
        for (auto& trackPtr : active->tracks)
            if (trackPtr != nullptr && trackPtr->runtime != nullptr)
                trackPtr->runtime->requestTransportDiscontinuity();
    endAudioRead();
}

void TimelineEngine::mix(float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    mix(nullptr, 0, outputChannels, channelCount, sampleCount);
}

void TimelineEngine::mix(const float* const* inputChannels, const int inputChannelCount,
                         float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    audioClockSample.fetch_add(static_cast<std::uint64_t>(sampleCount), std::memory_order_relaxed);
    callbackAudioStartSample.store(
        audioClockSample.load(std::memory_order_acquire) - static_cast<std::uint64_t>(sampleCount),
        std::memory_order_release);
    const auto blockPlaybackOffset =
        juce::jlimit(0, sampleCount, playbackBlockOffset.exchange(0, std::memory_order_acq_rel));
    lastMixPlaybackOffset.store(blockPlaybackOffset, std::memory_order_release);
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    if (active == nullptr) return;
    if (resetPlaybackPending.exchange(false, std::memory_order_acq_rel)) {
        clearPlaybackTrackState(*active);
        resetRecordingTrackState(*active);
    }
    if (seekPending.exchange(false, std::memory_order_acq_rel)) {
        timelineSample.store(pendingSeekSample.load(std::memory_order_acquire),
                             std::memory_order_release);
        clearPlaybackTrackState(*active);
        resetRecordingTrackState(*active);
    }
    applyPendingPanic(*active);
    const auto currentState = state.load(std::memory_order_acquire);
    if (currentState == State::stopped || currentState == State::starting) {
        for (auto& trackPtr : active->tracks)
            trackPtr->runtime->postEffectClipBuffer.clear(0, sampleCount);
        processLiveInstrumentTracks(*active, outputChannels, channelCount,
                                    timelineSample.load(std::memory_order_acquire), sampleCount);
        processLiveAudioTracks(*active, inputChannels, inputChannelCount, outputChannels,
                               channelCount, timelineSample.load(std::memory_order_acquire), 0,
                               sampleCount, true);
        return;
    }
    if (currentState != State::playing) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::stopping) {
        return;
    }
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = sampleCount - consumed;
        if (!active->tracks.empty()) {
            const auto bufferSize = active->tracks.front()->runtime->mixBuffer.getNumSamples();
            if (bufferSize > 0) chunk = std::min(chunk, bufferSize);
        }
        if (active->loopEnabled && position < active->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(active->loopEndSample - position));
        for (auto& trackPtr : active->tracks) {
            trackPtr->runtime->mixBuffer.clear(0, chunk);
            trackPtr->runtime->postEffectClipBuffer.clear(0, chunk);
        }
        for (auto& trackPtr : active->tracks) mixRange(*trackPtr, position, 0, chunk);
        for (auto& trackPtr : active->tracks) scheduleMidi(*active, *trackPtr, position, chunk);
        const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
        const auto captureSamples = captureBlockSamples.load(std::memory_order_acquire);
        const auto [captureWriteStart, captureWriteEnd] =
            ArrangementGraph::captureIntersection(consumed, chunk, captureStart, captureSamples);
        if (captureWriteEnd > captureWriteStart &&
            recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
            const auto callbackStart = audioClockSample.load(std::memory_order_acquire) -
                                       static_cast<std::uint64_t>(sampleCount);
            const auto localOffset = captureWriteStart - consumed;
            recordingCapture->setCaptureRange(
                callbackStart + static_cast<std::uint64_t>(captureWriteStart),
                callbackStart + static_cast<std::uint64_t>(captureWriteEnd),
                static_cast<std::uint64_t>(position) + static_cast<std::uint64_t>(localOffset),
                static_cast<std::uint64_t>(position) +
                    static_cast<std::uint64_t>(localOffset + captureWriteEnd - captureWriteStart));
        }
        processTracks(*active, inputChannels, inputChannelCount, outputChannels, channelCount,
                      position, consumed, chunk);
        position += chunk;
        consumed += chunk;
        // Decrement the capture budget so recording stops at the window end
        {
            auto remaining = captureBlockSamples.load(std::memory_order_acquire);
            if (remaining > 0)
                captureBlockSamples.store(remaining - std::min(chunk, remaining),
                                          std::memory_order_release);
        }
        if (active->loopEnabled && position >= active->loopEndSample) {
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                const auto callbackStart = audioClockSample.load(std::memory_order_acquire) -
                                           static_cast<std::uint64_t>(sampleCount);
                recordingCapture->markLoopBoundary(callbackStart +
                                                   static_cast<std::uint64_t>(consumed));
                for (auto& trackPtr : active->tracks) {
                    auto& track = *trackPtr;
                    auto& runtime = *track.runtime;
                    if (!runtime.armed || runtime.instrumentTrack ||
                        runtime.recordingCapture.state != RecordingCaptureState::capturing)
                        continue;
                    (void)recordingCapture->endTrackCapture(track.id, runtime.recordingCapture);
                    runtime.recordingCapture.state = RecordingCaptureState::idle;
                }
            }
            position = active->loopStartSample;
            recordingPassOrdinal.fetch_add(1, std::memory_order_relaxed);
            resetPlaybackTrackState(*active);
            discontinuity.fetch_add(1, std::memory_order_relaxed);
        }
    }
    timelineSample.store(position, std::memory_order_release);
}

}  // namespace riffra

