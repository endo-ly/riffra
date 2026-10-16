#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <utility>

#include "TimelineEngine.h"

namespace riffra {

namespace {

constexpr double kTransportFadeSeconds = 0.005;

}  // namespace

void TimelineEngine::applyMasterGain(float* const* outputChannels, const int channelCount,
                                     const int sampleCount, const float staticGainDb) noexcept {
    const auto* graph = realtime.graph;
    const auto preview = graph != nullptr ? graph->mixer.masterGainPreview.current() : std::nullopt;
    const auto fixedGain = juce::Decibels::decibelsToGain(preview.value_or(staticGainDb));
    if (graph == nullptr || preview.has_value() || graph->mixer.masterGainAutomation.empty()) {
        for (int channel = 0; channel < channelCount; ++channel)
            if (outputChannels[channel] != nullptr)
                juce::FloatVectorOperations::multiply(outputChannels[channel], fixedGain,
                                                      sampleCount);
        return;
    }
    int segmentIndex = 0;
    auto cursor = graph->mixer.masterGainAutomation.cursorAt(0);
    for (int sample = 0; sample < sampleCount; ++sample) {
        while (segmentIndex < realtime.metronomeTransportSegmentCount &&
               sample >=
                   realtime.metronomeTransportSegments[static_cast<std::size_t>(segmentIndex)]
                           .destinationStart +
                       realtime.metronomeTransportSegments[static_cast<std::size_t>(segmentIndex)]
                           .sampleCount)
            ++segmentIndex;
        auto gain = fixedGain;
        if (segmentIndex < realtime.metronomeTransportSegmentCount) {
            const auto& segment =
                realtime.metronomeTransportSegments[static_cast<std::size_t>(segmentIndex)];
            if (segment.active && sample >= segment.destinationStart) {
                auto position = segment.rangeStart + sample - segment.destinationStart;
                const auto loopLength = graph->loopEndSample - graph->loopStartSample;
                if (graph->loopEnabled && loopLength > 0 &&
                    segment.rangeStart < graph->loopEndSample && position >= graph->loopEndSample)
                    position =
                        graph->loopStartSample + (position - graph->loopEndSample) % loopLength;
                position -= static_cast<std::int64_t>(graph->summary.maximumLatencySamples);
                gain = juce::Decibels::decibelsToGain(cursor.valueAt(position, staticGainDb));
            }
        }
        for (int channel = 0; channel < channelCount; ++channel)
            if (outputChannels[channel] != nullptr) outputChannels[channel][sample] *= gain;
    }
}

void TimelineEngine::mixMetronome(float* const* outputChannels, const int channelCount,
                                  const int sampleCount) noexcept {
    if (sampleCount <= 0) return;
    const auto* active = realtime.graph;
    if (active == nullptr || !active->metronomeEnabled) return;
    const auto loopLength = active->loopEndSample - active->loopStartSample;
    const auto playbackOffset = juce::jlimit(0, sampleCount, realtime.lastMixPlaybackOffset);
    const auto countInRemaining = realtime.countInBlockStartRemainingSamples;
    const auto countingIn = realtime.recordingPhase == RecordingPhase::countingIn;
    constexpr std::int64_t clickSamples = 1'920;
    const auto segmentIndexAt = [this](const int destinationSample) noexcept {
        for (int index = 0; index < realtime.metronomeTransportSegmentCount; ++index) {
            const auto& segment =
                realtime.metronomeTransportSegments[static_cast<std::size_t>(index)];
            if (destinationSample < segment.destinationStart ||
                destinationSample >= segment.destinationStart + segment.sampleCount)
                continue;
            return index;
        }
        if (realtime.metronomeTransportSegmentCount > 0) {
            const auto lastIndex = realtime.metronomeTransportSegmentCount - 1;
            const auto& last =
                realtime.metronomeTransportSegments[static_cast<std::size_t>(lastIndex)];
            if (last.active && destinationSample >= last.destinationStart + last.sampleCount)
                return lastIndex;
        }
        return -1;
    };
    for (int sample = 0; sample < sampleCount; ++sample) {
        float value = 0.0f;
        if (countInRemaining > 0 && sample < (countingIn ? sampleCount : playbackOffset)) {
            const auto remaining = countInRemaining - sample;
            const auto offset =
                (realtime.countInBeatSamples - remaining % realtime.countInBeatSamples) %
                realtime.countInBeatSamples;
            if (offset >= 0 && offset < clickSamples) {
                const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                value = 0.11f * envelope;
            }
        } else {
            const auto segmentIndex = segmentIndexAt(sample);
            if (segmentIndex < 0) continue;
            const auto& segment =
                realtime.metronomeTransportSegments[static_cast<std::size_t>(segmentIndex)];
            const auto segmentOffset = std::max(0, sample - segment.destinationStart);
            const auto metronomeGain =
                juce::jlimit(0.0f, 1.0f, segment.gainStart + segment.gainStep * segmentOffset);
            auto position = segment.rangeStart + sample - segment.destinationStart;
            if (active->loopEnabled && loopLength > 0 && position >= active->loopEndSample)
                position =
                    active->loopStartSample + (position - active->loopEndSample) % loopLength;
            if (position >= 0) {
                const auto tick =
                    active->timebase.sampleToExactTick(position, active->outputSampleRate);
                const auto meter = active->timebase.meterAt(tick);
                const auto beatPosition = (meter.bar - std::floor(meter.bar)) * meter.numerator;
                const auto beat = static_cast<int>(std::floor(beatPosition + 1.0e-9));
                const auto beatTicks =
                    static_cast<double>(active->timebase.ppq) * 4.0 / meter.denominator;
                const auto beatTick = tick - (beatPosition - beat) * beatTicks;
                const auto offset =
                    position - active->timebase.tickToSample(
                                   static_cast<std::uint64_t>(std::llround(beatTick)),
                                   active->outputSampleRate);
                if (offset >= 0 && offset < clickSamples) {
                    const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                    const auto amplitude = beat == 0 ? 0.18f : 0.11f;
                    value = amplitude * envelope * metronomeGain;
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
    const auto pdcDelay = runtime.lowLatencyPdcBypassActive() ? runtime.timelinePdcDelaySamples : 0;
    const auto midiRangeStart = rangeStart - pdcDelay;
    MidiScheduler::schedule(runtime.midiClips, midiRangeStart, sampleCount, runtime.midiBuffer);
    runtime.instrumentEventCount = MidiScheduler::scheduleInstrumentEvents(
        runtime.midiClips, midiRangeStart, sampleCount, runtime.instrumentEvents.data(),
        runtime.instrumentEventOrdering.data(), runtime.instrumentEvents.size());
}

void TimelineEngine::processTracks(PreparedTimeline& prepared,
                                   const float* const* physicalInputChannels,
                                   const int physicalInputChannelCount,
                                   float* const* outputChannels, const int channelCount,
                                   const std::int64_t rangeStart, const int destinationStart,
                                   const int sampleCount, const float transportGainStart,
                                   const float transportGainStep) noexcept {
    processLiveAudioTracks(prepared, physicalInputChannels, physicalInputChannelCount, rangeStart,
                           destinationStart, sampleCount);
    clearSidechainInputs(prepared);
    for (std::size_t stageIndex = 0; stageIndex < prepared.processingStages.size(); ++stageIndex) {
        pool->run(prepared.processingStages[stageIndex],
                  {TrackStageKind::playback, &prepared, rangeStart, destinationStart, sampleCount,
                   transportGainStart, transportGainStep, true});
        mixSidechainStage(prepared, static_cast<int>(stageIndex), rangeStart, sampleCount, true);
    }
    mixProductionOutput(prepared, outputChannels, channelCount, rangeStart, destinationStart,
                        sampleCount, true);
    if (prepared.fadeOutSamples > 0) {
        const auto fadeStart = prepared.mixEndSample - prepared.fadeOutSamples;
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto position = rangeStart + sample -
                                  static_cast<std::int64_t>(prepared.summary.maximumLatencySamples);
            if (position < fadeStart) continue;
            const auto gain =
                prepared.fadeOutSamples == 1
                    ? 0.0f
                    : std::max(0.0f, 1.0f - static_cast<float>(position - fadeStart) /
                                                static_cast<float>(prepared.fadeOutSamples - 1));
            for (int channel = 0; channel < channelCount; ++channel)
                if (outputChannels[channel] != nullptr)
                    outputChannels[channel][destinationStart + sample] *= gain;
        }
    }
}

void TimelineEngine::processLiveAudioTracks(PreparedTimeline& prepared,
                                            const float* const* physicalInputChannels,
                                            const int physicalInputChannelCount,
                                            const std::int64_t rangeStart,
                                            const int destinationStart,
                                            const int sampleCount) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
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
            const auto captureStart = realtime.captureBlockOffset;
            const auto captureEnd = captureStart + realtime.captureBlockSamples;
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (runtime.armed) {
                auto& capture = runtime.recordingCapture;
                if (writeEnd > writeStart) {
                    const auto localOffset = writeStart - destinationStart;
                    const auto captureAudioStart =
                        realtime.callbackAudioStartSample + static_cast<std::uint64_t>(writeStart);
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

void TimelineEngine::clearSidechainInputs(PreparedTimeline& timeline) noexcept {
    for (auto& track : timeline.tracks) track->runtime->effects().clearSidechainBuffers();
    for (auto& bus : timeline.mixer.buses) bus->effects->clearSidechainBuffers();
    timeline.mixer.masterEffects->clearSidechainBuffers();
}

void TimelineEngine::mixSidechainStage(PreparedTimeline& timeline, const int stageIndex,
                                       const std::int64_t rangeStart, const int sampleCount,
                                       const bool playing) noexcept {
    for (auto* route : timeline.mixer.sidechainRoutesByStage[static_cast<std::size_t>(stageIndex)])
        route->mix(sampleCount, rangeStart, playing);
}

void TimelineEngine::processLiveInstrumentTracks(
    PreparedTimeline& prepared, float* const* outputChannels, const int channelCount,
    const std::int64_t rangeStart, const int destinationStart, const int sampleCount) noexcept {
    clearSidechainInputs(prepared);
    for (std::size_t stageIndex = 0; stageIndex < prepared.processingStages.size(); ++stageIndex) {
        pool->run(prepared.processingStages[stageIndex],
                  {TrackStageKind::live, &prepared, rangeStart, destinationStart, sampleCount, 1.0f,
                   0.0f, false});
        mixSidechainStage(prepared, static_cast<int>(stageIndex), rangeStart, sampleCount, false);
    }
    mixProductionOutput(prepared, outputChannels, channelCount, rangeStart, destinationStart,
                        sampleCount, false);
}

void TimelineEngine::processInstrumentTrack(const PreparedTimeline& prepared, Track& track,
                                            const int sampleCount,
                                            const juce::MidiBuffer* const timelineMidi,
                                            const std::int64_t rangeStart) noexcept {
    auto& runtime = *track.runtime;
    if (runtime.instrument() != nullptr) {
        const auto inputLatency = runtime.externalAudioSource == nullptr
                                      ? 0
                                      : runtime.externalAudioSource->routeLatencySamples;
        auto context = instrumentProcessContext(prepared, rangeStart - inputLatency, true);
        context.automation = runtime.deviceAutomation;
        context.timelineSample = rangeStart - inputLatency;
        context.timelineEvents = runtime.instrumentEvents.data();
        context.timelineEventCount = runtime.instrumentEventCount;
        if (runtime.externalAudioSource != nullptr) {
            context.externalInput =
                runtime.externalAudioSource->routedOutputBuffer.getArrayOfReadPointers();
            context.externalInputChannelCount = 2;
        }
        runtime.instrument()->process(runtime.mixBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                      timelineMidi, context);
    } else {
        runtime.mixBuffer.clear(0, sampleCount);
    }
    runtime.effects().process(
        runtime.mixBuffer.getArrayOfReadPointers(), 2,
        runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount, nullptr,
        runtime.deviceAutomation,
        rangeStart - runtime.routeLatencySamples + runtime.effects().latencySamples(), true);
}

void TimelineEngine::processLiveInstrumentTrack(const PreparedTimeline& prepared, Track& track,
                                                const int sampleCount,
                                                const std::int64_t rangeStart,
                                                const bool playing) noexcept {
    auto& runtime = *track.runtime;
    runtime.liveInputBuffer.clear(0, sampleCount);
    if (runtime.instrument() == nullptr ||
        (!runtime.liveMidiActive() && runtime.externalAudioSource == nullptr)) {
        runtime.processedBuffer.clear(0, sampleCount);
        return;
    }
    auto context = instrumentProcessContext(prepared, rangeStart, playing);
    context.automation = runtime.deviceAutomation;
    context.timelineSample = rangeStart;
    if (runtime.externalAudioSource != nullptr) {
        context.externalInput =
            runtime.externalAudioSource->routedOutputBuffer.getArrayOfReadPointers();
        context.externalInputChannelCount = 2;
    }
    runtime.instrument()->process(runtime.liveInputBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                  nullptr, context);
    runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                              runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount,
                              nullptr, runtime.deviceAutomation, rangeStart, playing);
    runtime.markLiveMidiProcessed(sampleCount);
}

void TimelineEngine::warmUpPluginDevices(const int sampleCount) noexcept {
    auto* graph = realtime.graph;
    if (graph == nullptr) return;
    clearSidechainInputs(*graph);
    for (auto& trackPtr : graph->tracks) {
        auto& runtime = *trackPtr->runtime;
        runtime.liveInputBuffer.clear(0, sampleCount);
        if (runtime.instrument() != nullptr && runtime.instrument()->vst3Rack() != nullptr)
            runtime.instrument()->process(runtime.liveInputBuffer.getArrayOfWritePointers(), 2,
                                          sampleCount, nullptr,
                                          instrumentProcessContext(*graph, 0, false));
        runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                                  runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                  sampleCount);
    }
    for (auto& bus : graph->mixer.buses) {
        bus->input.clear(0, sampleCount);
        bus->process(sampleCount, 0, false);
    }
    graph->mixer.masterInput.clear(0, sampleCount);
    graph->mixer.masterEffects->process(graph->mixer.masterInput.getArrayOfReadPointers(), 2,
                                        graph->mixer.masterOutput.getArrayOfWritePointers(), 2,
                                        sampleCount);
}

void TimelineEngine::mixTrackOutput(Track& track, const std::int64_t rangeStart,
                                    const int sampleCount, const float transportGainStart,
                                    const float transportGainStep, const bool playing) noexcept {
    auto& runtime = *track.runtime;
    const auto lowLatencyPdcBypassActive = runtime.lowLatencyPdcBypassActive();
    if (lowLatencyPdcBypassActive != runtime.postEffectLowLatencyBypassActive) {
        runtime.postEffectDelayBuffer.clear();
        runtime.postEffectDelayWritePosition = 0;
        runtime.postEffectLowLatencyBypassActive = lowLatencyPdcBypassActive;
    }
    const auto postEffectDelay = std::max<std::int64_t>(
        0, runtime.postEffectCompensationDelaySamples +
               (lowLatencyPdcBypassActive ? runtime.timelinePdcDelaySamples : 0));
    const auto postEffectDelaySize = runtime.postEffectDelayBuffer.getNumSamples();
    MixAutomationCursor mixCursor(
        runtime.volumeAutomation, runtime.panAutomation, rangeStart - runtime.routeLatencySamples,
        sampleCount, runtime.gainDb.load(std::memory_order_acquire),
        runtime.pan.load(std::memory_order_acquire), runtime.unityCenterStereo, playing,
        runtime.gainPreview.current(), runtime.panPreview.current());
    float peakLeft = 0.0f;
    float peakRight = 0.0f;
    double sumSquaresLeft = 0.0;
    double sumSquaresRight = 0.0;
    for (int processed = 0; processed < sampleCount; ++processed) {
        const auto gains = mixCursor.next();
        const auto leftGain = gains.left;
        const auto rightGain = gains.right;
        float left = runtime.processedBuffer.getSample(0, processed);
        float right = runtime.processedBuffer.getSample(1, processed);
        auto postEffectLeft = runtime.postEffectClipBuffer.getSample(0, processed);
        auto postEffectRight = runtime.postEffectClipBuffer.getSample(1, processed);
        if (postEffectDelaySize > 0) {
            const auto write = runtime.postEffectDelayWritePosition;
            runtime.postEffectDelayBuffer.setSample(0, static_cast<int>(write), postEffectLeft);
            runtime.postEffectDelayBuffer.setSample(1, static_cast<int>(write), postEffectRight);
            if (postEffectDelay > 0) {
                const auto read =
                    (write - postEffectDelay + postEffectDelaySize) % postEffectDelaySize;
                postEffectLeft = runtime.postEffectDelayBuffer.getSample(0, static_cast<int>(read));
                postEffectRight =
                    runtime.postEffectDelayBuffer.getSample(1, static_cast<int>(read));
            }
            runtime.postEffectDelayWritePosition = (write + 1) % postEffectDelaySize;
        }
        left += postEffectLeft;
        right += postEffectRight;
        const auto sampleGain =
            juce::jlimit(0.0f, 1.0f, transportGainStart + transportGainStep * processed);
        runtime.preFaderOutputBuffer.setSample(0, processed, left * sampleGain);
        runtime.preFaderOutputBuffer.setSample(1, processed, right * sampleGain);
        runtime.routedOutputBuffer.setSample(0, processed, left * leftGain);
        runtime.routedOutputBuffer.setSample(1, processed, right * rightGain);
        auto masterLeft = left * leftGain;
        auto masterRight = right * rightGain;
        const auto postFaderLeft = masterLeft * sampleGain;
        const auto postFaderRight = masterRight * sampleGain;
        runtime.trackOutputBuffer.setSample(0, processed, postFaderLeft);
        runtime.trackOutputBuffer.setSample(1, processed, postFaderRight);
        const auto contributionLeft = runtime.mainOutputAudible ? postFaderLeft : 0.0f;
        const auto contributionRight = runtime.mainOutputAudible ? postFaderRight : 0.0f;
        peakLeft = std::max(peakLeft, std::abs(contributionLeft));
        peakRight = std::max(peakRight, std::abs(contributionRight));
        sumSquaresLeft += static_cast<double>(contributionLeft) * contributionLeft;
        sumSquaresRight += static_cast<double>(contributionRight) * contributionRight;
    }
    if (sampleCount > 0) {
        runtime.meter.recordBlock(
            peakLeft, peakRight,
            static_cast<float>(std::sqrt(sumSquaresLeft / static_cast<double>(sampleCount))),
            static_cast<float>(std::sqrt(sumSquaresRight / static_cast<double>(sampleCount))));
    }
}

void TimelineEngine::mixProductionOutput(PreparedTimeline& prepared, float* const* outputChannels,
                                         const int channelCount, const std::int64_t rangeStart,
                                         const int destinationStart, const int sampleCount,
                                         const bool playing) noexcept {
    auto& mixer = prepared.mixer;
    mixer.masterInput.clear(0, sampleCount);
    for (auto& bus : mixer.buses) bus->input.clear(0, sampleCount);
    for (auto& route : mixer.trackRoutes) route->mix(sampleCount, rangeStart, playing);
    pool->runMixer(mixer.parallelChannels, sampleCount, rangeStart, playing);
    for (auto& route : mixer.busRoutes) route->mix(sampleCount, rangeStart, playing);
    mixer.masterEffects->process(mixer.masterInput.getArrayOfReadPointers(), 2,
                                 mixer.masterOutput.getArrayOfWritePointers(), 2, sampleCount,
                                 nullptr, mixer.masterDeviceAutomation,
                                 rangeStart - mixer.masterInputLatency, playing);
    for (int channel = 0; channel < std::min(2, channelCount); ++channel) {
        if (outputChannels[channel] != nullptr)
            juce::FloatVectorOperations::add(outputChannels[channel] + destinationStart,
                                             mixer.masterOutput.getReadPointer(channel),
                                             sampleCount);
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    clearPlaybackTrackState(prepared);
    for (auto& trackPtr : prepared.tracks) trackPtr->runtime->resetForTransportDiscontinuity();
}

void TimelineEngine::clearPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    prepared.restoreAutomation();
    prepared.mixer.reset();
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        runtime.mixBuffer.clear();
        runtime.processedBuffer.clear();
        runtime.postEffectClipBuffer.clear();
        runtime.midiBuffer.clear();
        runtime.instrumentEventCount = 0;
        runtime.routedOutputBuffer.clear();
        runtime.postEffectDelayBuffer.clear();
        runtime.postEffectDelayWritePosition = 0;
        runtime.postEffectLowLatencyBypassActive = false;
        runtime.resetTimelineInputDelay();
        runtime.effects().resetSidechainState();
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.runtime->recordingCapture);
    }
}

void TimelineEngine::beginTransportFade(const bool fadeIn, const double sampleRate) noexcept {
    const auto fadeSamples =
        std::max(1, static_cast<int>(std::ceil(std::max(1.0, sampleRate) * kTransportFadeSeconds)));
    if (fadeIn) {
        realtime.transportFadeStep =
            (1.0f - realtime.transportGain) / static_cast<float>(fadeSamples);
        if (realtime.transportGain >= 1.0f) {
            realtime.transportFadeRemaining = 0;
            realtime.renderTransportState = RenderTransportState::playing;
        } else {
            realtime.transportFadeRemaining = fadeSamples;
            realtime.renderTransportState = RenderTransportState::fadingIn;
        }
    } else {
        realtime.transportFadeStep = realtime.transportGain / static_cast<float>(fadeSamples);
        if (realtime.transportGain <= 0.0f) {
            realtime.transportFadeRemaining = 0;
            realtime.renderTransportState = RenderTransportState::stopped;
        } else {
            realtime.transportFadeRemaining = fadeSamples;
            realtime.renderTransportState = RenderTransportState::fadingOut;
        }
    }
}

void TimelineEngine::applyPendingSeek(PreparedTimeline& prepared) noexcept {
    if (!realtime.seekPending) return;
    realtime.seekPending = false;
    realtime.timelineSample = realtime.pendingSeekSample;
    resetPlaybackTrackState(prepared);
    if (realtime.recordingPhase != RecordingPhase::recording) resetRecordingTrackState(prepared);
}

void TimelineEngine::finishTransportFade(PreparedTimeline& prepared) noexcept {
    if (realtime.renderTransportState == RenderTransportState::fadingIn &&
        (realtime.transportGain >= 1.0f || realtime.transportFadeRemaining <= 0)) {
        realtime.transportGain = 1.0f;
        realtime.transportFadeStep = 0.0f;
        realtime.transportFadeRemaining = 0;
        realtime.renderTransportState = RenderTransportState::playing;
        return;
    }
    if (realtime.renderTransportState != RenderTransportState::fadingOut ||
        realtime.transportGain > 0.0f)
        return;
    realtime.transportGain = 0.0f;
    realtime.transportFadeStep = 0.0f;
    realtime.transportFadeRemaining = 0;
    if (realtime.transport == TransportState::paused) {
        realtime.renderTransportState = RenderTransportState::paused;
        return;
    }
    realtime.renderTransportState = RenderTransportState::stopped;
    resetPlaybackTrackState(prepared);
    if (realtime.recordingPhase != RecordingPhase::recording) resetRecordingTrackState(prepared);
    applyPendingSeek(prepared);
    realtime.seekRequestedWhileStopped = false;
}

void TimelineEngine::mix(float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    mix(nullptr, 0, outputChannels, channelCount, sampleCount);
}

void TimelineEngine::mix(const float* const* inputChannels, const int inputChannelCount,
                         float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    mixActiveGraph(inputChannels, inputChannelCount, outputChannels, channelCount, sampleCount);
    publishFrame(realtime);
}

void TimelineEngine::mixActiveGraph(const float* const* inputChannels, const int inputChannelCount,
                                    float* const* outputChannels, const int channelCount,
                                    const int sampleCount) noexcept {
    realtime.audioClockSample += static_cast<std::uint64_t>(sampleCount);
    realtime.callbackAudioStartSample =
        realtime.audioClockSample - static_cast<std::uint64_t>(sampleCount);
    const auto blockPlaybackOffset =
        juce::jlimit(0, sampleCount, std::exchange(realtime.playbackBlockOffset, 0));
    realtime.lastMixPlaybackOffset = blockPlaybackOffset;
    auto* active = realtime.graph;
    if (active == nullptr) return;
    realtime.metronomeTransportSegmentCount = 0;
    if (std::exchange(realtime.resetPlaybackPending, false)) {
        clearPlaybackTrackState(*active);
        resetRecordingTrackState(*active);
    }
    const auto currentState = realtime.transport;
    const auto sampleRate = active->outputSampleRate;
    if (realtime.seekPending && realtime.seekRequestedWhileStopped &&
        realtime.renderTransportState != RenderTransportState::fadingOut) {
        realtime.renderTransportState = RenderTransportState::stopped;
        realtime.transportGain = 0.0f;
        realtime.transportFadeStep = 0.0f;
        applyPendingSeek(*active);
        if (currentState == TransportState::paused)
            realtime.renderTransportState = RenderTransportState::paused;
        realtime.seekRequestedWhileStopped = false;
    }
    if (realtime.seekPending && realtime.renderTransportState != RenderTransportState::stopped &&
        realtime.renderTransportState != RenderTransportState::fadingOut) {
        beginTransportFade(false, sampleRate);
    } else if (currentState == TransportState::playing &&
               (realtime.renderTransportState == RenderTransportState::stopped ||
                realtime.renderTransportState == RenderTransportState::paused)) {
        if (realtime.seekPending) applyPendingSeek(*active);
        beginTransportFade(true, sampleRate);
    } else if (currentState == TransportState::paused &&
               realtime.renderTransportState == RenderTransportState::stopped) {
        realtime.renderTransportState = RenderTransportState::paused;
        realtime.transportGain = 0.0f;
    } else if (currentState != TransportState::playing &&
               (realtime.renderTransportState == RenderTransportState::playing ||
                realtime.renderTransportState == RenderTransportState::fadingIn))
        beginTransportFade(false, sampleRate);

    const auto processStoppedRange = [&](const int destinationStart, const int rangeSamples,
                                         const std::int64_t rangeStart) noexcept {
        if (rangeSamples <= 0) return;
        for (int offset = 0; offset < rangeSamples;) {
            const auto chunk = std::min(rangeSamples - offset, active->preparedBlockSize);
            for (auto& track : active->tracks) track->runtime->postEffectClipBuffer.clear(0, chunk);
            processLiveAudioTracks(*active, inputChannels, inputChannelCount, rangeStart + offset,
                                   destinationStart + offset, chunk);
            processLiveInstrumentTracks(*active, outputChannels, channelCount, rangeStart + offset,
                                        destinationStart + offset, chunk);
            offset += chunk;
        }
    };

    const auto appendMetronomeSegment = [&](const int destinationStart, const int rangeSamples,
                                            const std::int64_t rangeStart, const bool segmentActive,
                                            const float gainStart, const float gainStep) noexcept {
        if (rangeSamples <= 0) return;
        if (realtime.metronomeTransportSegmentCount > 0) {
            auto& previous = realtime.metronomeTransportSegments[static_cast<std::size_t>(
                realtime.metronomeTransportSegmentCount - 1)];
            auto nextPosition = previous.rangeStart + previous.sampleCount;
            const auto loopLength = active->loopEndSample - active->loopStartSample;
            if (active->loopEnabled && loopLength > 0 && previous.active &&
                previous.rangeStart < active->loopEndSample &&
                nextPosition >= active->loopEndSample)
                nextPosition =
                    active->loopStartSample + (nextPosition - active->loopEndSample) % loopLength;
            if (previous.active == segmentActive &&
                previous.destinationStart + previous.sampleCount == destinationStart &&
                nextPosition == rangeStart && previous.gainStep == gainStep &&
                std::abs(previous.gainStart + previous.gainStep * previous.sampleCount -
                         gainStart) < 0.000001f) {
                previous.sampleCount += rangeSamples;
                return;
            }
        }
        if (realtime.metronomeTransportSegmentCount >=
            static_cast<int>(realtime.metronomeTransportSegments.size()))
            return;
        auto& segment = realtime.metronomeTransportSegments[static_cast<std::size_t>(
            realtime.metronomeTransportSegmentCount++)];
        segment.rangeStart = rangeStart;
        segment.destinationStart = destinationStart;
        segment.sampleCount = rangeSamples;
        segment.gainStart = gainStart;
        segment.gainStep = gainStep;
        segment.active = segmentActive;
    };

    if (realtime.renderTransportState == RenderTransportState::paused) {
        for (int channel = 0; channel < channelCount; ++channel)
            if (outputChannels[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannels[channel], sampleCount);
        return;
    }
    if (realtime.renderTransportState == RenderTransportState::stopped) {
        applyPendingSeek(*active);
        const auto stoppedPosition = realtime.timelineSample;
        appendMetronomeSegment(0, sampleCount, stoppedPosition, false, 0.0f, 0.0f);
        processStoppedRange(0, sampleCount, stoppedPosition);
        return;
    }
    if (currentState == TransportState::faulted) return;
    auto position = realtime.timelineSample;
    if (realtime.recordingPhase == RecordingPhase::stopping) {
        return;
    }
    const auto captureWindowStart = realtime.captureBlockOffset;
    const auto captureWindowSamples = realtime.captureBlockSamples;
    auto capturedSamplesInCallback = 0;
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = std::min(sampleCount - consumed, active->preparedBlockSize);
        if (realtime.renderTransportState == RenderTransportState::paused) {
            for (int channel = 0; channel < channelCount; ++channel)
                if (outputChannels[channel] != nullptr)
                    juce::FloatVectorOperations::clear(outputChannels[channel] + consumed,
                                                       sampleCount - consumed);
            appendMetronomeSegment(consumed, sampleCount - consumed, position, false, 0.0f, 0.0f);
            break;
        }
        if (realtime.renderTransportState == RenderTransportState::stopped) {
            appendMetronomeSegment(consumed, sampleCount - consumed, position, false, 0.0f, 0.0f);
            processStoppedRange(consumed, sampleCount - consumed, position);
            break;
        }
        const auto fadingIn = realtime.renderTransportState == RenderTransportState::fadingIn;
        const auto fadingOut = realtime.renderTransportState == RenderTransportState::fadingOut;
        if (fadingIn || fadingOut) chunk = std::min(chunk, realtime.transportFadeRemaining);
        if (!active->tracks.empty()) {
            const auto bufferSize = active->tracks.front()->runtime->mixBuffer.getNumSamples();
            if (bufferSize > 0) chunk = std::min(chunk, bufferSize);
        }
        if (active->loopEnabled && position < active->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(active->loopEndSample - position));
        const auto nextTimebaseChange = std::upper_bound(
            active->timebaseChangeSamples.begin(), active->timebaseChangeSamples.end(), position);
        if (nextTimebaseChange != active->timebaseChangeSamples.end())
            chunk = std::min<std::int64_t>(chunk, *nextTimebaseChange - position);
        if (chunk <= 0) break;
        appendMetronomeSegment(consumed, chunk, position, true, realtime.transportGain,
                               fadingIn    ? realtime.transportFadeStep
                               : fadingOut ? -realtime.transportFadeStep
                                           : 0.0f);
        for (auto& trackPtr : active->tracks) {
            trackPtr->runtime->mixBuffer.clear(0, chunk);
            trackPtr->runtime->postEffectClipBuffer.clear(0, chunk);
        }
        const auto [captureWriteStart, captureWriteEnd] = ArrangementGraph::captureIntersection(
            consumed, chunk, captureWindowStart, captureWindowSamples);
        if (captureWriteEnd > captureWriteStart &&
            realtime.recordingPhase == RecordingPhase::recording) {
            const auto callbackStart =
                realtime.audioClockSample - static_cast<std::uint64_t>(sampleCount);
            const auto localOffset = captureWriteStart - consumed;
            recordingCapture->setCaptureRange(
                callbackStart + static_cast<std::uint64_t>(captureWriteStart),
                callbackStart + static_cast<std::uint64_t>(captureWriteEnd),
                static_cast<std::uint64_t>(position) + static_cast<std::uint64_t>(localOffset),
                static_cast<std::uint64_t>(position) +
                    static_cast<std::uint64_t>(localOffset + captureWriteEnd - captureWriteStart));
        }
        processTracks(*active, inputChannels, inputChannelCount, outputChannels, channelCount,
                      position, consumed, chunk, realtime.transportGain,
                      fadingIn    ? realtime.transportFadeStep
                      : fadingOut ? -realtime.transportFadeStep
                                  : 0.0f);
        capturedSamplesInCallback += captureWriteEnd - captureWriteStart;
        position += chunk;
        consumed += chunk;
        realtime.timelineSample = position;
        if (active->loopEnabled && position >= active->loopEndSample) {
            if (realtime.recordingPhase == RecordingPhase::recording) {
                const auto callbackStart =
                    realtime.audioClockSample - static_cast<std::uint64_t>(sampleCount);
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
            ++realtime.recordingPassOrdinal;
            resetPlaybackTrackState(*active);
            ++realtime.discontinuity;
        }
        realtime.timelineSample = position;
        if (fadingIn || fadingOut) {
            realtime.transportGain = juce::jlimit(
                0.0f, 1.0f,
                realtime.transportGain +
                    (fadingIn ? realtime.transportFadeStep : -realtime.transportFadeStep) * chunk);
            realtime.transportFadeRemaining -= chunk;
            if (realtime.transportFadeRemaining <= 0) {
                realtime.transportGain = fadingIn ? 1.0f : 0.0f;
                finishTransportFade(*active);
                position = realtime.timelineSample;
                if (realtime.renderTransportState == RenderTransportState::stopped &&
                    currentState == TransportState::playing) {
                    beginTransportFade(true, sampleRate);
                }
            }
        }
    }
    realtime.captureBlockSamples -=
        std::min(realtime.captureBlockSamples, capturedSamplesInCallback);
}

}  // namespace riffra
