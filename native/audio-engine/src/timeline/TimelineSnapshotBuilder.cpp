#include "TimelineSnapshotBuilder.h"

#include <algorithm>
#include <cmath>
#include <limits>

#include "ArrangementGraph.h"
#include "MidiScheduler.h"
#include "TrackRuntime.h"
#include "instruments/SonalloyInstrumentRuntime.h"
#include "instruments/Vst3InstrumentRuntime.h"

namespace riffra {
namespace {

constexpr int kReadAheadSamples = 32768;

juce::String instrumentId(const std::optional<InstrumentSpec>& instrument) {
    if (!instrument.has_value()) return {};
    return std::visit([](const auto& value) { return value.id; }, *instrument);
}

}  // namespace

TimelineSnapshotBuilder::TimelineSnapshotBuilder(TimelineEngine& engine) noexcept
    : engine(engine) {}

bool TimelineSnapshotBuilder::build(const TimelineSnapshotSpec& snapshot,
                                    juce::AudioFormatManager& formats,
                                    const double outputSampleRate, const int maximumBlockSize,
                                    std::unique_ptr<TimelineEngine::PreparedTimeline>& prepared,
                                    bool& monitorLiveInputState,
                                    std::uint32_t& monitoringInputChannelsState,
                                    bool& armedInstrumentTrackState, juce::String& error) {
    using Clip = TimelineEngine::Clip;
    using PreparedTimeline = TimelineEngine::PreparedTimeline;
    using Track = TimelineEngine::Track;

    prepared.reset();
    monitorLiveInputState = false;
    monitoringInputChannelsState = 0;
    armedInstrumentTrackState = false;
    if (!std::isfinite(outputSampleRate) || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }

    const auto& graph = snapshot.graph;
    const auto processingMode =
        engine.offlineMode ? PluginProcessingMode::offline : PluginProcessingMode::realtime;
    prepared = std::make_unique<PreparedTimeline>();
    prepared->projectId = snapshot.projectId;
    prepared->revision = snapshot.revision;
    prepared->timebase.ppq = graph.timebase.ppq;
    prepared->timebase.bpm = graph.timebase.bpm;
    prepared->outputSampleRate = outputSampleRate;
    prepared->preparedBlockSize = maximumBlockSize;
    prepared->masterGainDb = static_cast<float>(graph.masterGainDb);
    const auto beatTicks =
        static_cast<double>(prepared->timebase.ppq) * 4.0 / graph.timebase.timeSignatureDenominator;
    prepared->beatSamples = prepared->timebase.tickToSample(
        static_cast<std::uint64_t>(std::llround(beatTicks)), outputSampleRate);
    prepared->beatsPerBar = graph.timebase.timeSignatureNumerator;
    prepared->timeSignatureNumerator = graph.timebase.timeSignatureNumerator;
    prepared->timeSignatureDenominator = graph.timebase.timeSignatureDenominator;
    prepared->metronomeEnabled = graph.metronomeEnabled;
    prepared->loopEnabled = graph.loopRange.enabled;
    prepared->loopStartSample =
        prepared->timebase.tickToSample(graph.loopRange.startTick, outputSampleRate);
    prepared->loopEndSample =
        prepared->timebase.tickToSample(graph.loopRange.endTick, outputSampleRate);
    if (graph.punchRange.has_value()) {
        prepared->punchStartSample =
            prepared->timebase.tickToSample(graph.punchRange->startTick, outputSampleRate);
        prepared->punchEndSample =
            prepared->timebase.tickToSample(graph.punchRange->endTick, outputSampleRate);
        prepared->punchEnabled = true;
    }

    std::int64_t maximumPluginDelay = 0;
    for (const auto& trackSpec : graph.tracks) {
        auto track = std::make_unique<Track>();
        track->runtime = std::make_unique<TrackRuntime>();
        track->id = trackSpec.id;
        track->runtime->outputSampleRate = outputSampleRate;
        track->runtime->preparedBlockSize = maximumBlockSize;
        track->runtime->instrumentTrack = trackSpec.kind == TrackKindSpec::instrument;
        track->runtime->armed = trackSpec.armed;
        track->runtime->midiDeviceId = trackSpec.midiInput.deviceId.value_or(juce::String());
        track->runtime->midiChannel = trackSpec.midiInput.channel.has_value()
                                          ? static_cast<int>(*trackSpec.midiInput.channel)
                                          : 0;
        armedInstrumentTrackState |= track->runtime->instrumentTrack && track->runtime->armed;
        track->runtime->gainDb.store(static_cast<float>(trackSpec.gainDb),
                                     std::memory_order_release);
        track->runtime->pan.store(static_cast<float>(trackSpec.pan), std::memory_order_release);
        track->runtime->muted = trackSpec.muted;
        track->runtime->solo = trackSpec.solo;
        prepared->hasSolo = prepared->hasSolo || track->runtime->solo;
        track->runtime->monitorInput = ArrangementGraph::shouldMonitorAudioInput(
            trackSpec.monitoring, track->runtime->armed, track->runtime->instrumentTrack);
        track->runtime->setLowLatencyMonitoring(
            track->runtime->instrumentTrack
                ? (track->runtime->armed || engine.isLiveMidiTarget(track->id))
                : track->runtime->monitorInput);
        if (track->runtime->monitorInput) monitorLiveInputState = true;
        track->runtime->audioInputChannel =
            trackSpec.audioInput.has_value() ? static_cast<int>(trackSpec.audioInput->channelIndex)
                                             : -1;
        if (track->runtime->monitorInput && track->runtime->audioInputChannel >= 0)
            monitoringInputChannelsState |=
                std::uint32_t{1} << static_cast<unsigned>(track->runtime->audioInputChannel);

        std::vector<AutomationRuntime::Point> volumeAutomation;
        std::vector<AutomationRuntime::Point> panAutomation;
        volumeAutomation.reserve(trackSpec.volumeAutomation.size());
        panAutomation.reserve(trackSpec.panAutomation.size());
        for (const auto& point : trackSpec.volumeAutomation)
            volumeAutomation.push_back(
                {prepared->timebase.tickToSample(point.tick, outputSampleRate),
                 static_cast<float>(point.value)});
        for (const auto& point : trackSpec.panAutomation)
            panAutomation.push_back({prepared->timebase.tickToSample(point.tick, outputSampleRate),
                                     static_cast<float>(point.value)});
        track->runtime->volumeAutomation.setPoints(std::move(volumeAutomation));
        track->runtime->panAutomation.setPoints(std::move(panAutomation));

        track->effects = trackSpec.effects;
        track->instrument = trackSpec.instrument;
        track->instrumentDeviceId = instrumentId(track->instrument);
        auto sameRuntimeTopology = false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (!engine.runtimeDevicesNeedReprepare.load(std::memory_order_acquire) &&
                engine.timeline != nullptr) {
                const auto existing =
                    std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                                 [&track](const auto& item) { return item->id == track->id; });
                if (existing != engine.timeline->tracks.end() &&
                    sameEffectTopology((*existing)->effects, track->effects) &&
                    sameInstrumentTopology((*existing)->instrument, track->instrument) &&
                    (*existing)->runtime->outputSampleRate == track->runtime->outputSampleRate &&
                    (*existing)->runtime->preparedBlockSize == track->runtime->preparedBlockSize) {
                    sameRuntimeTopology = true;
                    track->runtime->pluginDelaySamples = (*existing)->runtime->pluginDelaySamples;
                    track->runtime->pluginTailSamples = (*existing)->runtime->pluginTailSamples;
                    track->reuseRuntimeDevices = (*existing)->effects == track->effects &&
                                                 (*existing)->instrument == track->instrument;
                }
            }
        }
        if (!track->reuseRuntimeDevices &&
            !track->runtime->effects().load(track->effects, outputSampleRate, maximumBlockSize,
                                            processingMode, error, track->id + "/track-effect"))
            return false;
        if (track->instrument.has_value() && !track->reuseRuntimeDevices) {
            const auto roleError = [&track, &error](const juce::String& role,
                                                    const juce::String& detail) {
                error = track->id + " device " + track->instrumentDeviceId + " failed at " + role +
                        ": " + detail;
                return false;
            };
            if (const auto* vst3 = std::get_if<Vst3InstrumentSpec>(&*track->instrument)) {
                juce::String runtimeError;
                track->runtime->setInstrument(
                    Vst3InstrumentRuntime::create(vst3->path, outputSampleRate, maximumBlockSize,
                                                  processingMode, vst3->state, runtimeError));
                if (track->runtime->instrument() == nullptr)
                    return roleError("instrument", runtimeError);
            } else {
                const auto& internal = std::get<InternalInstrumentSpec>(*track->instrument);
                juce::String runtimeError;
                track->runtime->setInstrument(SonalloyInstrumentRuntime::create(
                    internal.definitionJson, internal.definitionBaseDir, outputSampleRate,
                    maximumBlockSize, runtimeError));
                if (track->runtime->instrument() == nullptr)
                    return roleError("instrument", runtimeError);
                track->runtime->instrument()->setBypassed(internal.bypassed);
            }
        }
        if (!sameRuntimeTopology) {
            track->runtime->pluginDelaySamples = track->runtime->pluginLatencySamples();
            track->runtime->pluginTailSamples = track->runtime->totalPluginTailSamples();
        }
        maximumPluginDelay = std::max(maximumPluginDelay, track->runtime->pluginDelaySamples);

        for (const auto& clipSpec : trackSpec.audioClips) {
            const auto path = clipSpec.path;
            auto reader =
                std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(juce::File(path)));
            if (reader == nullptr || reader->lengthInSamples <= 0 || reader->sampleRate <= 0.0) {
                error = "Timeline source could not be opened: " + path;
                return false;
            }
            if (std::abs(static_cast<double>(clipSpec.sourceSampleRate) - reader->sampleRate) >
                    0.5 ||
                clipSpec.sourceEndFrame > static_cast<std::uint64_t>(reader->lengthInSamples)) {
                error = "Timeline source metadata does not match the audio file: " + path;
                return false;
            }
            auto clip = std::make_unique<Clip>();
            clip->id = clipSpec.id;
            clip->processingStage = clipSpec.takeVariant == TakeVariantSpec::processed
                                        ? ProcessingStage::PostEffects
                                        : ProcessingStage::PreEffects;
            clip->sourceSampleRate = reader->sampleRate;
            clip->sourceStartFrame = static_cast<std::int64_t>(clipSpec.sourceStartFrame);
            clip->sourceEndFrame = static_cast<std::int64_t>(clipSpec.sourceEndFrame);
            clip->startSample =
                prepared->timebase.tickToSample(clipSpec.startTick, outputSampleRate);
            clip->durationSamples = static_cast<std::int64_t>(
                std::llround(static_cast<double>(clipSpec.durationFrames) * outputSampleRate /
                             clipSpec.durationSampleRate));
            clip->fadeInSamples = static_cast<std::int64_t>(
                std::llround(static_cast<double>(clipSpec.fadeInFrames) * outputSampleRate /
                             clipSpec.durationSampleRate));
            clip->fadeOutSamples = static_cast<std::int64_t>(
                std::llround(static_cast<double>(clipSpec.fadeOutFrames) * outputSampleRate /
                             clipSpec.durationSampleRate));
            clip->fadeShape = static_cast<int>(clipSpec.fadeShape);
            clip->gain = juce::Decibels::decibelsToGain(static_cast<float>(clipSpec.gainDb));
            clip->pan = static_cast<float>(clipSpec.pan);
            const auto panAngle = (clip->pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
            clip->leftGain = clip->gain * std::cos(panAngle);
            clip->rightGain = clip->gain * std::sin(panAngle);
            clip->loop = clipSpec.loopEnabled;
            clip->muted = clipSpec.muted;
            clip->readerSource =
                std::make_unique<juce::AudioFormatReaderSource>(reader.release(), true);
            clip->positionableSource = clip->readerSource.get();
            if (!engine.offlineMode) {
                clip->bufferingSource = std::make_unique<juce::BufferingAudioSource>(
                    clip->readerSource.get(), engine.readAheadThread, false, kReadAheadSamples, 2);
                clip->positionableSource = clip->bufferingSource.get();
            }
            clip->resamplingSource =
                std::make_unique<juce::ResamplingAudioSource>(clip->positionableSource, false, 2);
            clip->resamplingSource->setResamplingRatio(clip->sourceSampleRate / outputSampleRate);
            clip->resamplingSource->prepareToPlay(maximumBlockSize, outputSampleRate);
            clip->scratch.setSize(2, maximumBlockSize, false, true, false);
            track->clips.push_back(std::move(clip));
        }

        for (const auto& clipSpec : trackSpec.midiClips) {
            MidiClip midiClip;
            midiClip.startTick = clipSpec.startTick;
            midiClip.durationTicks = clipSpec.durationTicks;
            midiClip.loop = clipSpec.loopEnabled;
            midiClip.muted = clipSpec.muted;
            midiClip.notes.reserve(clipSpec.notes.size());
            for (const auto& noteSpec : clipSpec.notes)
                midiClip.notes.push_back({noteSpec.startTick, noteSpec.durationTicks, noteSpec.note,
                                          noteSpec.velocity, noteSpec.channel});
            midiClip.events.reserve(clipSpec.events.size());
            for (const auto& eventSpec : clipSpec.events) {
                const auto kind =
                    eventSpec.kind == MidiEventKindSpec::controlChange ? "controlChange"
                    : eventSpec.kind == MidiEventKindSpec::pitchBend   ? "pitchBend"
                                                                       : "channelPressure";
                midiClip.events.push_back(
                    {kind, eventSpec.tick, eventSpec.channel, eventSpec.data1, eventSpec.data2});
            }
            MidiScheduler::CompiledMidiClip compiled;
            if (!MidiScheduler::compile(midiClip, prepared->timebase, outputSampleRate, compiled,
                                        error))
                return false;
            track->runtime->midiClips.push_back(std::move(compiled));
        }
        track->runtime->midiEventCapacity =
            MidiScheduler::maximumEventsPerBlock(track->runtime->midiClips, maximumBlockSize);
        if (!MidiScheduler::prepareBuffer(track->runtime->midiBuffer,
                                          track->runtime->midiEventCapacity)) {
            error = "Timeline MIDI requires an audio buffer larger than the native runtime allows.";
            return false;
        }
        if (!track->runtime->prepareTimelineMidiCapacity(track->runtime->midiEventCapacity, error))
            return false;
        track->runtime->mixBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->postEffectClipBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->liveInputBuffer.setSize(2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }

    for (auto& track : prepared->tracks) {
        track->runtime->compensationDelaySamples = ArrangementGraph::compensationDelay(
            maximumPluginDelay, track->runtime->pluginDelaySamples);
        track->runtime->delayBuffer.setSize(
            2, static_cast<int>(track->runtime->compensationDelaySamples + maximumBlockSize + 1),
            false, true, false);
        track->runtime->delayBuffer.clear();
        track->runtime->postEffectCompensationDelaySamples = maximumPluginDelay;
        track->runtime->postEffectDelayBuffer.setSize(
            2,
            static_cast<int>(track->runtime->postEffectCompensationDelaySamples + maximumBlockSize +
                             1),
            false, true, false);
        track->runtime->postEffectDelayBuffer.clear();
    }
    return true;
}

}  // namespace riffra
