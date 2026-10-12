#include "TimelineSnapshotBuilder.h"

#include <algorithm>
#include <cmath>
#include <limits>

#include "ArrangementGraph.h"
#include "MidiScheduler.h"
#include "TrackRuntime.h"
#include "contract/MixerGraphValidation.h"
#include "instruments/SonalloyInstrumentRuntime.h"
#include "instruments/Vst3InstrumentRuntime.h"

namespace riffra {
namespace {

constexpr int kReadAheadSamples = 32768;
// Device instances are prepared with headroom so later graphs with denser MIDI
// can keep sharing them; a graph that needs more receives new instances.
constexpr std::size_t kSharedDeviceMidiCapacity = 256;

juce::String instrumentId(const std::optional<InstrumentSpec>& instrument) {
    if (!instrument.has_value()) return {};
    return std::visit([](const auto& value) { return value.id; }, *instrument);
}

bool matchesExecutedEffects(const PluginChain& chain,
                            const std::vector<PluginDeviceSpec>& effects) {
    for (const auto& effect : effects) {
        const auto* rack = chain.findDevice(effect.id);
        if (rack == nullptr) return false;
        juce::String error;
        const auto state = rack->persistedState(error);
        if (!state || *state != effect.state) return false;
    }
    return true;
}

}  // namespace

TimelineSnapshotBuilder::TimelineSnapshotBuilder(TimelineEngine& engine) noexcept
    : engine(engine) {}

bool TimelineSnapshotBuilder::build(const TimelineSnapshotSpec& snapshot,
                                    juce::AudioFormatManager& formats,
                                    const double outputSampleRate, const int maximumBlockSize,
                                    std::unique_ptr<PreparedTimeline>& prepared,
                                    juce::String& error) {
    using Clip = PreparedTimeline::Clip;
    using Track = PreparedTimeline::Track;

    prepared.reset();
    if (!std::isfinite(outputSampleRate) || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }

    const auto& graph = snapshot.graph;
    if (!validateMixerGraph(graph, error)) return false;
    const auto processingMode =
        engine.offlineMode ? PluginProcessingMode::offline : PluginProcessingMode::realtime;
    prepared = std::make_unique<PreparedTimeline>();
    prepared->projectId = snapshot.projectId;
    prepared->revision = snapshot.revision;
    prepared->timebase.ppq = graph.timebase.ppq;
    prepared->timebase.tempoChanges = graph.timebase.tempoChanges;
    prepared->timebase.timeSignatureChanges = graph.timebase.timeSignatureChanges;
    prepared->outputSampleRate = outputSampleRate;
    for (const auto& change : graph.timebase.tempoChanges)
        prepared->timebaseChangeSamples.push_back(
            prepared->timebase.tickToSample(change.tick, outputSampleRate));
    for (const auto& change : graph.timebase.timeSignatureChanges)
        prepared->timebaseChangeSamples.push_back(
            prepared->timebase.tickToSample(change.tick, outputSampleRate));
    std::sort(prepared->timebaseChangeSamples.begin(), prepared->timebaseChangeSamples.end());
    prepared->timebaseChangeSamples.erase(
        std::unique(prepared->timebaseChangeSamples.begin(), prepared->timebaseChangeSamples.end()),
        prepared->timebaseChangeSamples.end());
    prepared->preparedBlockSize = maximumBlockSize;
    prepared->masterGainDb = static_cast<float>(graph.masterGainDb);
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

    for (const auto& trackSpec : graph.tracks) {
        auto track = std::make_unique<Track>();
        track->runtime = std::make_unique<TrackRuntime>();
        track->id = trackSpec.id;
        track->runtime->outputSampleRate = outputSampleRate;
        track->runtime->preparedBlockSize = maximumBlockSize;
        track->runtime->instrumentTrack = trackSpec.kind == TrackKindSpec::instrument;
        track->runtime->armed = trackSpec.armed;
        track->runtime->midiDeviceId = trackSpec.midiInput.deviceId.value_or(juce::String());
        track->runtime->midiSourceIndex = engine.midiSources.indexFor(track->runtime->midiDeviceId);
        track->runtime->midiChannel = trackSpec.midiInput.channel.has_value()
                                          ? static_cast<int>(*trackSpec.midiInput.channel)
                                          : 0;
        track->runtime->key =
            engine.graphRegistry.access([&track](ControlGraphRegistry::State& graphs) {
                return graphs.trackKeys.keyFor(track->id);
            });
        track->runtime->gainDb.store(static_cast<float>(trackSpec.gainDb),
                                     std::memory_order_release);
        track->runtime->pan.store(static_cast<float>(trackSpec.pan), std::memory_order_release);
        track->runtime->muted = trackSpec.muted;
        track->runtime->solo = trackSpec.solo;
        prepared->hasSolo = prepared->hasSolo || track->runtime->solo;
        track->runtime->monitorInput = trackSpec.monitorInput;
        track->runtime->audioInputChannel =
            trackSpec.audioInput.has_value() ? static_cast<int>(trackSpec.audioInput->channelIndex)
                                             : -1;

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

        std::uint64_t nextTimelineNoteId = 0;
        std::uint64_t noteIdStride = 0;
        for (const auto& clip : trackSpec.midiClips) noteIdStride += clip.notes.size();
        for (const auto& clipSpec : trackSpec.midiClips) {
            MidiClip midiClip;
            midiClip.startTick = clipSpec.startTick;
            midiClip.durationTicks = clipSpec.durationTicks;
            midiClip.loop = clipSpec.loopEnabled;
            midiClip.muted = clipSpec.muted;
            midiClip.directInstrumentEvents =
                trackSpec.instrument.has_value() &&
                std::holds_alternative<InternalInstrumentSpec>(*trackSpec.instrument);
            midiClip.noteIdBase = nextTimelineNoteId;
            nextTimelineNoteId += clipSpec.notes.size();
            midiClip.instrumentControlEvents = clipSpec.instrumentControlEvents;
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
            const auto originalStart = midiClip.startTick;
            const auto originalNoteBase = midiClip.noteIdBase;
            std::uint64_t iteration = 0;
            do {
                const auto lastTempoTick = prepared->timebase.tempoChanges.back().tick;
                midiClip.loop = clipSpec.loopEnabled && midiClip.startTick >= lastTempoTick;
                midiClip.noteIdBase = originalNoteBase + iteration * noteIdStride;
                MidiScheduler::CompiledMidiClip compiled;
                if (!MidiScheduler::compile(midiClip, prepared->timebase, outputSampleRate,
                                            compiled, error))
                    return false;
                compiled.noteIdStride = noteIdStride;
                track->runtime->midiClips.push_back(std::move(compiled));
                if (!clipSpec.loopEnabled || midiClip.loop) break;
                if (++iteration > 100'000 ||
                    iteration > (std::numeric_limits<std::uint64_t>::max() - originalStart) /
                                    midiClip.durationTicks) {
                    error = "Timeline loop preparation exceeds the supported event range.";
                    return false;
                }
                midiClip.startTick = originalStart + iteration * midiClip.durationTicks;
            } while (true);
        }
        track->runtime->midiEventCapacity =
            MidiScheduler::maximumEventsPerBlock(track->runtime->midiClips, maximumBlockSize);
        track->runtime->instrumentEvents.resize(track->runtime->midiEventCapacity);
        track->runtime->instrumentEventOrdering.resize(track->runtime->midiEventCapacity);
        if (!MidiScheduler::prepareBuffer(track->runtime->midiBuffer,
                                          track->runtime->midiEventCapacity)) {
            error = "Timeline MIDI requires an audio buffer larger than the native runtime allows.";
            return false;
        }

        // Device instances already published are shared, never prepared again:
        // the audio thread may be processing them.
        engine.graphRegistry.access([&track](ControlGraphRegistry::State& graphs) {
            if (graphs.devicesNeedReprepare || graphs.latestCommitted == nullptr) return;
            const auto* existing = graphs.latestCommitted->findTrack(track->id);
            if (existing == nullptr || !sameEffectTopology(existing->effects, track->effects) ||
                !sameInstrumentTopology(existing->instrument, track->instrument) ||
                existing->runtime->outputSampleRate != track->runtime->outputSampleRate ||
                existing->runtime->preparedBlockSize != track->runtime->preparedBlockSize)
                return;
            track->runtime->pluginDelaySamples = existing->runtime->pluginDelaySamples;
            track->runtime->pluginTailSamples = existing->runtime->pluginTailSamples;
            track->reuseRuntimeDevices =
                (existing->effects == track->effects ||
                 matchesExecutedEffects(existing->runtime->effects(), track->effects)) &&
                existing->instrument == track->instrument &&
                existing->runtime->timelineMidiCapacity() >= track->runtime->midiEventCapacity;
            if (track->reuseRuntimeDevices) track->runtime->shareDevicesWith(*existing->runtime);
        });
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
        if (!track->reuseRuntimeDevices &&
            !track->runtime->prepareTimelineMidiCapacity(
                std::max(track->runtime->midiEventCapacity, kSharedDeviceMidiCapacity), error))
            return false;
        if (auto* sonalloy =
                dynamic_cast<SonalloyInstrumentRuntime*>(track->runtime->instrument())) {
            for (auto& clip : track->runtime->midiClips) {
                for (auto& event : clip.events) {
                    if (event.instrumentEvent &&
                        event.instrumentEvent->event_type == SONALLOY_EVENT_PARAMETER_CHANGE &&
                        !sonalloy->prepareParameterEvent(*event.instrumentEvent, event.parameter,
                                                         error)) {
                        error = track->id + ": " + error;
                        return false;
                    }
                }
            }
        }
        track->runtime->pluginDelaySamples = track->runtime->pluginLatencySamples();
        track->runtime->pluginTailSamples = track->runtime->totalPluginTailSamples();

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

        track->runtime->mixBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->trackOutputBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->routedOutputBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->preFaderOutputBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->unityCenterStereo = trackSpec.panLaw == "unityCenterStereo";
        track->runtime->postEffectClipBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->runtime->liveInputBuffer.setSize(2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }

    std::vector<int> levels(prepared->tracks.size(), -1);
    for (std::size_t index = 0; index < prepared->tracks.size(); ++index) {
        auto& runtime = *prepared->tracks[index]->runtime;
        auto* sonalloy = dynamic_cast<SonalloyInstrumentRuntime*>(runtime.instrument());
        const auto& sourceId = snapshot.graph.tracks[index].externalAudioSourceTrackId;
        if ((sonalloy != nullptr && sonalloy->requiredInputChannels() > 0) !=
            sourceId.has_value()) {
            error = prepared->tracks[index]->id +
                    ": instrument external audio input does not match the route.";
            return false;
        }
        if (sourceId) {
            auto* source = prepared->findTrack(*sourceId);
            if (source == nullptr || source == prepared->tracks[index].get()) {
                error = prepared->tracks[index]->id +
                        ": external audio source is missing or refers to itself.";
                return false;
            }
            runtime.externalAudioSource = source->runtime.get();
        }
    }
    std::size_t remaining = levels.size();
    for (int level = 0; remaining > 0; ++level) {
        std::vector<Track*> stage;
        for (std::size_t index = 0; index < levels.size(); ++index) {
            if (levels[index] >= 0) continue;
            const auto* source = prepared->tracks[index]->runtime->externalAudioSource;
            auto sourceReady = source == nullptr;
            if (source != nullptr) {
                for (std::size_t candidate = 0; candidate < levels.size(); ++candidate)
                    if (prepared->tracks[candidate]->runtime.get() == source)
                        sourceReady = levels[candidate] >= 0 && levels[candidate] < level;
            }
            if (!sourceReady) continue;
            stage.push_back(prepared->tracks[index].get());
            levels[index] = level;
            --remaining;
        }
        if (stage.empty()) {
            error = "External audio routes contain a cycle.";
            return false;
        }
        for (auto* track : stage) {
            auto& runtime = *track->runtime;
            const auto sourceLatency = runtime.externalAudioSource == nullptr
                                           ? 0
                                           : runtime.externalAudioSource->routeLatencySamples;
            runtime.routeLatencySamples = sourceLatency + runtime.pluginDelaySamples;
            runtime.postEffectCompensationDelaySamples = runtime.routeLatencySamples;
            for (auto& clip : runtime.midiClips) clip.startSample += sourceLatency;
        }
        prepared->processingStages.push_back(std::move(stage));
    }

    auto& summary = prepared->summary;
    summary.trackCount = prepared->tracks.size();
    for (const auto& track : prepared->tracks) {
        const auto& runtime = *track->runtime;
        summary.pluginCount += static_cast<std::uint64_t>(runtime.effects().size());
        summary.maximumLatencySamples = std::max<std::uint64_t>(
            summary.maximumLatencySamples, static_cast<std::uint64_t>(runtime.routeLatencySamples));
        if (runtime.armed) summary.armedTrackIds.push_back(track->id);
        if (runtime.instrument() != nullptr) ++summary.instrumentRuntimeCount;
        summary.armedInstrumentTrack |= runtime.instrumentTrack && runtime.armed;
        summary.monitorLiveInput |= runtime.monitorInput;
        if (runtime.monitorInput && runtime.audioInputChannel >= 0)
            summary.monitoringInputChannels |= std::uint32_t{1}
                                               << static_cast<unsigned>(runtime.audioInputChannel);
    }

    const auto originalTimebaseChanges = prepared->timebaseChangeSamples;
    for (auto& track : prepared->tracks) {
        if (track->runtime->externalAudioSource != nullptr)
            for (const auto change : originalTimebaseChanges)
                prepared->timebaseChangeSamples.push_back(
                    change + track->runtime->externalAudioSource->routeLatencySamples);
        track->runtime->postEffectDelayBuffer.setSize(
            2,
            static_cast<int>(track->runtime->postEffectCompensationDelaySamples + maximumBlockSize +
                             1),
            false, true, false);
        track->runtime->postEffectDelayBuffer.clear();
    }
    if (!prepareMixer(graph, *prepared, processingMode, error)) return false;
    std::sort(prepared->timebaseChangeSamples.begin(), prepared->timebaseChangeSamples.end());
    prepared->timebaseChangeSamples.erase(
        std::unique(prepared->timebaseChangeSamples.begin(), prepared->timebaseChangeSamples.end()),
        prepared->timebaseChangeSamples.end());
    prepared->mixEndSample =
        prepared->timebase.tickToSample(graph.musicalEndTick, outputSampleRate);
    for (const auto& track : prepared->tracks) {
        for (const auto& clip : track->clips)
            prepared->mixEndSample =
                std::max(prepared->mixEndSample, clip->startSample + clip->durationSamples);
    }
    for (const auto& track : graph.tracks)
        for (const auto& clip : track.midiClips) {
            const auto end = prepared->timebase.tickToSample(clip.startTick + clip.durationTicks,
                                                             outputSampleRate);
            prepared->mixEndSample = std::max(prepared->mixEndSample, end);
            for (const auto& event : clip.instrumentControlEvents)
                if (event.tick == clip.durationTicks)
                    prepared->mixEndSample = std::max(prepared->mixEndSample, end + 1);
            for (const auto& note : clip.notes)
                if (note.startTick + note.durationTicks == clip.durationTicks)
                    prepared->mixEndSample = std::max(prepared->mixEndSample, end + 1);
        }
    prepared->mixEndSample +=
        static_cast<std::int64_t>(std::llround(graph.tailSeconds * outputSampleRate));
    prepared->fadeOutSamples =
        static_cast<std::int64_t>(std::llround(graph.fadeOutSeconds * outputSampleRate));
    if (prepared->fadeOutSamples > prepared->mixEndSample) {
        error = "Mix fade duration exceeds the final mix duration.";
        return false;
    }
    return true;
}

bool TimelineSnapshotBuilder::prepareMixer(const ExecutionGraph& graph, PreparedTimeline& prepared,
                                           const PluginProcessingMode mode, juce::String& error) {
    auto& mixer = prepared.mixer;
    const auto blockSize = prepared.preparedBlockSize;
    const auto sampleRate = prepared.outputSampleRate;
    mixer.masterInput.setSize(2, blockSize);
    mixer.masterOutput.setSize(2, blockSize);
    prepared.hasSolo = std::any_of(graph.tracks.begin(), graph.tracks.end(),
                                   [](const auto& track) { return track.solo; }) ||
                       std::any_of(graph.mixer.buses.begin(), graph.mixer.buses.end(),
                                   [](const auto& bus) { return bus.solo; });
    for (const auto& spec : graph.mixer.buses) {
        auto bus = std::make_unique<MixerChannel>();
        bus->id = spec.id;
        bus->effectSpecs = spec.effects;
        bus->gainDb.store(static_cast<float>(spec.gainDb));
        bus->pan.store(static_cast<float>(spec.pan));
        bus->unityCenterStereo = spec.panLaw == "unityCenterStereo";
        bus->audible = !prepared.hasSolo && !spec.muted;
        bus->input.setSize(2, blockSize);
        bus->output.setSize(2, blockSize);
        bool reused = false;
        engine.graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
            const auto* previous = graphs.latestCommitted;
            if (previous == nullptr || graphs.devicesNeedReprepare ||
                previous->preparedBlockSize != blockSize ||
                previous->outputSampleRate != sampleRate)
                return;
            const auto* existing = previous->mixer.findBus(spec.id);
            if (existing != nullptr && sameEffectTopology(existing->effectSpecs, spec.effects) &&
                (existing->effectSpecs == spec.effects ||
                 matchesExecutedEffects(*existing->effects, spec.effects))) {
                bus->effects = existing->effects;
                reused = true;
            }
        });
        if (!reused && !bus->effects->load(spec.effects, sampleRate, blockSize, mode, error,
                                           spec.id + "/bus-effect"))
            return false;
        prepared.summary.pluginCount += static_cast<std::uint64_t>(bus->effects->size());
        mixer.parallelChannels.push_back(bus.get());
        mixer.buses.push_back(std::move(bus));
    }
    mixer.masterEffectSpecs = graph.mixer.masterEffects;
    bool masterReused = false;
    engine.graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        const auto* previous = graphs.latestCommitted;
        if (previous != nullptr && !graphs.devicesNeedReprepare &&
            previous->preparedBlockSize == blockSize && previous->outputSampleRate == sampleRate &&
            sameEffectTopology(previous->mixer.masterEffectSpecs, mixer.masterEffectSpecs) &&
            (previous->mixer.masterEffectSpecs == mixer.masterEffectSpecs ||
             matchesExecutedEffects(*previous->mixer.masterEffects, mixer.masterEffectSpecs))) {
            mixer.masterEffects = previous->mixer.masterEffects;
            masterReused = true;
        }
    });
    if (!masterReused && !mixer.masterEffects->load(mixer.masterEffectSpecs, sampleRate, blockSize,
                                                    mode, error, "master-effect"))
        return false;
    prepared.summary.pluginCount += static_cast<std::uint64_t>(mixer.masterEffects->size());
    const auto busSpec = [&](const juce::String& id) -> const MixBusSpec* {
        for (const auto& spec : graph.mixer.buses)
            if (spec.id == id) return &spec;
        return nullptr;
    };
    const auto addTrackRoute = [&](TrackRuntime& track, MixerChannel* destination,
                                   const bool preFader, const float gain, const bool audible,
                                   const juce::String& sendId) {
        auto route = std::make_unique<MixerRoute>();
        route->track = &track;
        route->source = preFader ? &track.preFaderOutputBuffer : &track.trackOutputBuffer;
        route->destination = destination != nullptr ? &destination->input : &mixer.masterInput;
        route->gain.store(gain);
        route->audible = audible;
        route->sendId = sendId;
        if (destination != nullptr) {
            destination->inputLatency =
                std::max(destination->inputLatency, track.routeLatencySamples);
            if (audible) destination->sources.push_back(&track);
            if (audible && !busSpec(destination->id)->muted) destination->audible = true;
        } else {
            mixer.masterInputLatency =
                std::max(mixer.masterInputLatency, track.routeLatencySamples);
        }
        mixer.trackRoutes.push_back(std::move(route));
    };
    for (std::size_t index = 0; index < graph.tracks.size(); ++index) {
        const auto& spec = graph.tracks[index];
        auto& track = *prepared.tracks[index]->runtime;
        const auto* group = spec.outputBusId ? busSpec(*spec.outputBusId) : nullptr;
        const auto selected = !prepared.hasSolo || spec.solo || (group != nullptr && group->solo);
        track.mainOutputAudible = selected && !spec.muted && (group == nullptr || !group->muted);
        addTrackRoute(track, spec.outputBusId ? mixer.findBus(*spec.outputBusId) : nullptr, false,
                      1.0f, selected && !spec.muted, {});
        for (const auto& send : graph.mixer.sends) {
            if (send.trackId != spec.id) continue;
            const auto* target = busSpec(send.returnBusId);
            addTrackRoute(track, mixer.findBus(send.returnBusId),
                          send.position == SendPositionSpec::preFader,
                          juce::Decibels::decibelsToGain(static_cast<float>(send.levelDb)),
                          send.enabled && !spec.muted && (selected || target->solo), send.id);
        }
    }
    for (auto& bus : mixer.buses) {
        bus->outputLatency = bus->inputLatency + bus->effects->latencySamples();
        mixer.masterInputLatency = std::max(mixer.masterInputLatency, bus->outputLatency);
    }
    const auto prepareDelay = [&](MixRouteDelay& delay, const std::int64_t samples) {
        if (samples < 0 || samples >= std::numeric_limits<int>::max()) {
            error = "mix route latency exceeds the supported buffer size";
            return false;
        }
        delay.prepare(static_cast<int>(samples));
        return true;
    };
    for (auto& route : mixer.trackRoutes) {
        const auto target =
            std::find_if(mixer.buses.begin(), mixer.buses.end(),
                         [&](const auto& bus) { return route->destination == &bus->input; });
        const auto targetLatency =
            target != mixer.buses.end() ? (*target)->inputLatency : mixer.masterInputLatency;
        if (!prepareDelay(route->delay, targetLatency - route->track->routeLatencySamples))
            return false;
    }
    for (auto& bus : mixer.buses) {
        auto route = std::make_unique<MixerRoute>();
        route->bus = bus.get();
        route->source = &bus->output;
        route->destination = &mixer.masterInput;
        if (!prepareDelay(route->delay, mixer.masterInputLatency - bus->outputLatency))
            return false;
        mixer.busRoutes.push_back(std::move(route));
    }
    const auto finalLatency = mixer.masterInputLatency + mixer.masterEffects->latencySamples();
    prepared.summary.maximumLatencySamples = static_cast<std::uint64_t>(finalLatency);
    return true;
}

}  // namespace riffra
