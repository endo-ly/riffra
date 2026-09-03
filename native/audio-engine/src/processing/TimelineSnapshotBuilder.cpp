#include "TimelineSnapshotBuilder.h"

#include <algorithm>
#include <array>
#include <cmath>

#include "ArrangementGraph.h"

namespace riffra {
namespace {

constexpr int kReadAheadSamples = 32768;

juce::String pluginTopologySignature(const juce::var& values) {
    juce::Array<juce::var> topology;
    const auto append = [&topology](const juce::var& value) {
        if (!value.isObject()) return;
        auto* device = new juce::DynamicObject();
        device->setProperty("id", value.getProperty("id", {}));
        device->setProperty("kind", value.getProperty("kind", {}));
        device->setProperty("path", value.getProperty("path", {}));
        device->setProperty("disabledPlaceholder", value.getProperty("disabledPlaceholder", false));
        topology.add(juce::var(device));
    };
    if (values.isArray()) {
        for (const auto& value : *values.getArray()) append(value);
    } else if (values.isObject()) {
        append(values);
    }
    return juce::JSON::toString(juce::var(topology), false);
}

bool requiredNumber(const juce::var& object, const juce::Identifier& name, double& value) {
    const auto property = object.getProperty(name, {});
    if (!property.isInt() && !property.isInt64() && !property.isDouble()) return false;
    value = static_cast<double>(property);
    return std::isfinite(value);
}

}  // namespace

TimelineSnapshotBuilder::TimelineSnapshotBuilder(TimelineEngine& engine) noexcept
    : engine(engine) {}

bool TimelineSnapshotBuilder::build(const juce::var& snapshot, juce::AudioFormatManager& formats,
                                    const double outputSampleRate, const int maximumBlockSize,
                                    std::unique_ptr<TimelineEngine::PreparedTimeline>& prepared,
                                    bool& monitorLiveInputState,
                                    std::uint32_t& monitoringInputChannelsState,
                                    bool& armedInstrumentTrackState, juce::String& error) {
    using Clip = TimelineEngine::Clip;
    using MidiClip = TimelineEngine::MidiClip;
    using MidiEvent = TimelineEngine::MidiEvent;
    using MidiNote = TimelineEngine::MidiNote;
    using PreparedTimeline = TimelineEngine::PreparedTimeline;
    using Track = TimelineEngine::Track;

    prepared.reset();
    monitorLiveInputState = false;
    monitoringInputChannelsState = 0;
    armedInstrumentTrackState = false;
    if (!snapshot.isObject() || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }
    prepared = std::make_unique<PreparedTimeline>();
    prepared->revision =
        static_cast<std::uint64_t>(static_cast<juce::int64>(snapshot.getProperty("revision", -1)));
    const auto unavailableClipIds = snapshot.getProperty("unavailableClipIds", {});
    if (unavailableClipIds.isArray()) prepared->unavailableClipIds = *unavailableClipIds.getArray();
    const auto missingDeviceIds = snapshot.getProperty("missingDeviceIds", {});
    if (missingDeviceIds.isArray()) prepared->missingDeviceIds = *missingDeviceIds.getArray();
    const auto timebase = snapshot.getProperty("timebase", {});
    double ppq = 0.0;
    if (!timebase.isObject() || !requiredNumber(timebase, "ppq", ppq) ||
        !requiredNumber(timebase, "bpm", prepared->timebase.bpm) || ppq != 960.0 ||
        prepared->timebase.bpm < 20.0 || prepared->timebase.bpm > 400.0) {
        error = "Timeline snapshot has an invalid timebase.";
        return false;
    }
    prepared->timebase.ppq = static_cast<std::uint32_t>(ppq);
    prepared->outputSampleRate = outputSampleRate;
    prepared->preparedBlockSize = maximumBlockSize;
    const auto denominator = static_cast<int>(timebase.getProperty("timeSignatureDenominator", 4));
    const auto numerator = static_cast<int>(timebase.getProperty("timeSignatureNumerator", 4));
    if (denominator <= 0 || numerator <= 0) {
        error = "Timeline snapshot has an invalid time signature.";
        return false;
    }
    const auto beatTicks = static_cast<double>(prepared->timebase.ppq) * 4.0 / denominator;
    prepared->beatSamples = prepared->timebase.tickToSample(
        static_cast<std::uint64_t>(std::llround(beatTicks)), outputSampleRate);
    prepared->beatsPerBar = numerator;
    prepared->metronomeEnabled = static_cast<bool>(snapshot.getProperty("metronomeEnabled", false));

    const auto loopRange = snapshot.getProperty("loopRange", {});
    if (loopRange.isObject()) {
        prepared->loopEnabled = static_cast<bool>(loopRange.getProperty("enabled", false));
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("endTick", 0)));
        prepared->loopStartSample = prepared->timebase.tickToSample(startTick, outputSampleRate);
        prepared->loopEndSample = prepared->timebase.tickToSample(endTick, outputSampleRate);
        if (prepared->loopEnabled && prepared->loopEndSample <= prepared->loopStartSample) {
            error = "Timeline loop range must have a positive duration.";
            return false;
        }
    }

    const auto punchRange = snapshot.getProperty("punchRange", {});
    if (punchRange.isObject()) {
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(punchRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(punchRange.getProperty("endTick", 0)));
        prepared->punchStartSample = prepared->timebase.tickToSample(startTick, outputSampleRate);
        prepared->punchEndSample = prepared->timebase.tickToSample(endTick, outputSampleRate);
        if (prepared->punchEndSample <= prepared->punchStartSample) {
            error = "Timeline punch range must have a positive duration.";
            return false;
        }
        prepared->punchEnabled = true;
    }

    const auto tracks = snapshot.getProperty("tracks", {});
    if (!tracks.isArray()) {
        error = "Timeline snapshot tracks must be an array.";
        return false;
    }
    std::int64_t maximumPluginDelay = 0;
    for (const auto& trackValue : *tracks.getArray()) {
        if (!trackValue.isObject()) {
            error = "Timeline track must be an object.";
            return false;
        }
        auto track = std::make_unique<Track>();
        track->id = trackValue.getProperty("id", {}).toString();
        track->outputSampleRate = outputSampleRate;
        track->instrument = trackValue.getProperty("kind", {}).toString() == "instrument";
        track->armed = static_cast<bool>(trackValue.getProperty("armed", false));
        const auto midiInput = trackValue.getProperty("midiInput", {});
        if (midiInput.isObject()) {
            track->midiDeviceId = midiInput.getProperty("deviceId", {}).toString();
            track->midiChannel = static_cast<int>(midiInput.getProperty("channel", 0));
        }
        armedInstrumentTrackState |= track->instrument && track->armed;
        if (track->id.isEmpty()) {
            error = "Timeline track requires an id.";
            return false;
        }
        track->gainDb =
            juce::jlimit(-90.0f, 24.0f, static_cast<float>(trackValue.getProperty("gainDb", 0.0)));
        track->pan =
            juce::jlimit(-1.0f, 1.0f, static_cast<float>(trackValue.getProperty("pan", 0.0)));
        track->muted = static_cast<bool>(trackValue.getProperty("muted", false));
        track->solo = static_cast<bool>(trackValue.getProperty("solo", false));
        const auto monitoring = trackValue.getProperty("monitoring", {}).toString();
        track->monitorInput =
            ArrangementGraph::shouldMonitorAudioInput(monitoring, track->armed, track->instrument);
        track->liveEffectRuntimeRequired = track->instrument || track->monitorInput;
        track->recordingEffectRuntimeRequired = !track->instrument && track->armed;
        if (track->monitorInput) monitorLiveInputState = true;
        const auto audioInput = trackValue.getProperty("audioInput", {});
        if (audioInput.isObject())
            track->audioInputChannel = static_cast<int>(audioInput.getProperty("channelIndex", -1));
        if (track->monitorInput && track->audioInputChannel >= 0 && track->audioInputChannel < 32)
            monitoringInputChannelsState |= std::uint32_t{1}
                                            << static_cast<unsigned>(track->audioInputChannel);

        const auto automation =
            trackValue.getProperty("automation", juce::var(juce::Array<juce::var>{}));
        if (!automation.isArray()) {
            error = "Timeline track automation must be an array.";
            return false;
        }
        for (const auto& lane : *automation.getArray()) {
            if (!lane.isObject()) {
                error = "Timeline Automation Lane must be an object.";
                return false;
            }
            const auto parameter = lane.getProperty("parameter", {}).toString();
            const auto pointValues = lane.getProperty("points", {});
            if ((parameter != "volume" && parameter != "pan") || !pointValues.isArray()) {
                error = "Timeline Automation Lane has an invalid parameter or point list.";
                return false;
            }
            auto& destination =
                parameter == "volume" ? track->volumeAutomation : track->panAutomation;
            for (const auto& pointValue : *pointValues.getArray()) {
                if (!pointValue.isObject()) {
                    error = "Timeline Automation Point must be an object.";
                    return false;
                }
                const auto tick = static_cast<std::uint64_t>(
                    static_cast<juce::int64>(pointValue.getProperty("tick", 0)));
                const auto value = static_cast<float>(pointValue.getProperty("value", 0.0));
                if (!std::isfinite(value)) {
                    error = "Timeline Automation Point must have a finite value.";
                    return false;
                }
                destination.push_back({
                    prepared->timebase.tickToSample(tick, outputSampleRate),
                    parameter == "volume" ? juce::jlimit(-90.0f, 24.0f, value)
                                          : juce::jlimit(-1.0f, 1.0f, value),
                });
            }
        }

        const auto rack = trackValue.getProperty("rack", {});
        const auto instrument = trackValue.getProperty("instrument", {});
        const auto devices =
            rack.isObject() ? rack.getProperty("devices", {}) : juce::var(juce::Array<juce::var>{});
        track->effectTopologySignature = pluginTopologySignature(devices);
        track->instrumentTopologySignature = pluginTopologySignature(instrument);
        track->effectState = devices;
        track->instrumentState = instrument;
        track->instrumentDeviceId =
            instrument.isObject() ? instrument.getProperty("id", {}).toString() : juce::String();
        juce::var existingEffectState;
        juce::var existingInstrumentState;
        auto sameRuntimeTopology = false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (!engine.runtimeDevicesNeedReprepare.load(std::memory_order_acquire) &&
                engine.timeline != nullptr) {
                const auto existing =
                    std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                                 [&track](const auto& item) { return item->id == track->id; });
                if (existing != engine.timeline->tracks.end() &&
                    (*existing)->effectTopologySignature == track->effectTopologySignature &&
                    (*existing)->instrumentTopologySignature ==
                        track->instrumentTopologySignature &&
                    (*existing)->liveEffectRuntimeRequired == track->liveEffectRuntimeRequired &&
                    (*existing)->recordingEffectRuntimeRequired ==
                        track->recordingEffectRuntimeRequired) {
                    sameRuntimeTopology = true;
                    existingEffectState = (*existing)->effectState;
                    existingInstrumentState = (*existing)->instrumentState;
                    track->pluginDelaySamples = (*existing)->pluginDelaySamples;
                    track->pluginTailSamples = (*existing)->pluginTailSamples;
                }
            }
        }
        if (sameRuntimeTopology) {
            track->reuseRuntimeDevices = juce::JSON::toString(existingEffectState, false) ==
                                             juce::JSON::toString(track->effectState, false) &&
                                         juce::JSON::toString(existingInstrumentState, false) ==
                                             juce::JSON::toString(track->instrumentState, false);
        }
        if (rack.isObject()) {
            if (!track->reuseRuntimeDevices &&
                !track->effectChain.load(devices, outputSampleRate, maximumBlockSize, error,
                                         track->id + "/timeline-effect"))
                return false;
            if (!track->reuseRuntimeDevices && track->liveEffectRuntimeRequired &&
                !track->liveEffectChain.load(devices, outputSampleRate, maximumBlockSize, error,
                                             track->id + "/live-effect"))
                return false;
            if (!track->reuseRuntimeDevices && track->recordingEffectRuntimeRequired &&
                !track->recordingCapture.effectChain.load(devices, outputSampleRate,
                                                          maximumBlockSize, error,
                                                          track->id + "/recording-effect"))
                return false;
        }
        if (instrument.isObject() &&
            !static_cast<bool>(instrument.getProperty("disabledPlaceholder", false)) &&
            !track->reuseRuntimeDevices) {
            const auto path = instrument.getProperty("path", {}).toString();
            const auto loadInstrumentRack = [&](std::unique_ptr<PluginRack>& rack,
                                                const juce::String& runtimeRole) {
                rack = std::make_unique<PluginRack>();
                if (const auto loadError = rack->load(path, outputSampleRate, maximumBlockSize)) {
                    error = track->id + " device " + track->instrumentDeviceId + " failed at " +
                            runtimeRole + "/" + loadError->scope + ": " + loadError->message;
                    return false;
                }
                if (!rack->applyPersistedState(instrument, error)) {
                    error = track->id + " device " + track->instrumentDeviceId + " failed at " +
                            runtimeRole + "/stateApply: " + error;
                    return false;
                }
                return true;
            };
            if (!loadInstrumentRack(track->instrumentRack, "timeline-instrument") ||
                !loadInstrumentRack(track->liveInstrumentRack, "live-instrument"))
                return false;
        }
        if (!track->reuseRuntimeDevices) {
            track->pluginDelaySamples =
                track->effectChain.latencySamples() +
                (track->instrumentRack != nullptr ? track->instrumentRack->latencySamples() : 0);
            track->pluginTailSamples =
                track->effectChain.tailSamples() +
                (track->instrumentRack != nullptr ? track->instrumentRack->tailSamples() : 0);
        }
        maximumPluginDelay = std::max(maximumPluginDelay, track->pluginDelaySamples);

        const auto clips = trackValue.getProperty("audioClips", {});
        if (!clips.isArray()) {
            error = "Timeline track audioClips must be an array.";
            return false;
        }
        for (const auto& value : *clips.getArray()) {
            if (!value.isObject()) {
                error = "Timeline clip must be an object.";
                return false;
            }
            const auto path = value.getProperty("path", {}).toString();
            auto reader =
                std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(juce::File(path)));
            if (reader == nullptr || reader->lengthInSamples <= 0 || reader->sampleRate <= 0.0) {
                error = "Timeline source could not be opened: " + path;
                return false;
            }
            auto clip = std::make_unique<Clip>();
            clip->id = value.getProperty("clipId", {}).toString();
            const auto declaredSourceRate =
                static_cast<double>(value.getProperty("sourceSampleRate", 0.0));
            clip->sourceSampleRate = reader->sampleRate;
            clip->sourceStartFrame =
                static_cast<juce::int64>(value.getProperty("sourceStartFrame", 0));
            clip->sourceEndFrame = static_cast<juce::int64>(value.getProperty("sourceEndFrame", 0));
            const auto durationFrames =
                static_cast<juce::int64>(value.getProperty("durationFrames", 0));
            const auto durationRate =
                static_cast<double>(value.getProperty("durationSampleRate", 0.0));
            if (clip->id.isEmpty() || declaredSourceRate <= 0.0 ||
                std::abs(declaredSourceRate - reader->sampleRate) > 0.5 ||
                clip->sourceStartFrame < 0 || clip->sourceEndFrame <= clip->sourceStartFrame ||
                clip->sourceEndFrame > reader->lengthInSamples || durationFrames <= 0 ||
                durationRate <= 0.0) {
                error = "Timeline clip has an invalid frame range: " + clip->id;
                return false;
            }
            const auto startTick = static_cast<std::uint64_t>(
                static_cast<juce::int64>(value.getProperty("startTick", 0)));
            clip->startSample = prepared->timebase.tickToSample(startTick, outputSampleRate);
            clip->durationSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(durationFrames) * outputSampleRate / durationRate));
            const auto fadeInFrames =
                static_cast<juce::int64>(value.getProperty("fadeInFrames", 0));
            const auto fadeOutFrames =
                static_cast<juce::int64>(value.getProperty("fadeOutFrames", 0));
            clip->fadeInSamples = static_cast<std::int64_t>(
                std::llround(static_cast<double>(fadeInFrames) * outputSampleRate / durationRate));
            clip->fadeOutSamples = static_cast<std::int64_t>(
                std::llround(static_cast<double>(fadeOutFrames) * outputSampleRate / durationRate));
            clip->fadeShape =
                juce::jlimit(0, 2, static_cast<int>(value.getProperty("fadeShape", 1)));
            clip->gain = juce::Decibels::decibelsToGain(
                static_cast<float>(value.getProperty("gainDb", 0.0)));
            clip->pan =
                juce::jlimit(-1.0f, 1.0f, static_cast<float>(value.getProperty("pan", 0.0)));
            clip->loop = static_cast<bool>(value.getProperty("loopEnabled", false));
            clip->muted = static_cast<bool>(value.getProperty("muted", false));
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
        const auto midiClips = trackValue.getProperty("midiClips", {});
        if (!midiClips.isArray()) {
            error = "Timeline track midiClips must be an array.";
            return false;
        }
        for (const auto& value : *midiClips.getArray()) {
            if (!value.isObject()) {
                error = "Timeline MIDI clip must be an object.";
                return false;
            }
            MidiClip midiClip;
            midiClip.startTick = static_cast<std::uint64_t>(
                static_cast<juce::int64>(value.getProperty("startTick", 0)));
            midiClip.durationTicks = static_cast<std::uint64_t>(
                static_cast<juce::int64>(value.getProperty("durationTicks", 0)));
            midiClip.loop = static_cast<bool>(value.getProperty("loopEnabled", false));
            midiClip.muted = static_cast<bool>(value.getProperty("muted", false));
            if (midiClip.durationTicks == 0) {
                error = "Timeline MIDI clip must have a positive duration.";
                return false;
            }
            const auto notes = value.getProperty("notes", {});
            if (!notes.isArray()) {
                error = "Timeline MIDI clip notes must be an array.";
                return false;
            }
            for (const auto& noteValue : *notes.getArray()) {
                if (!noteValue.isObject()) {
                    error = "Timeline MIDI note must be an object.";
                    return false;
                }
                MidiNote note;
                note.startTick = static_cast<std::uint64_t>(
                    static_cast<juce::int64>(noteValue.getProperty("startTick", 0)));
                note.durationTicks = static_cast<std::uint64_t>(
                    static_cast<juce::int64>(noteValue.getProperty("durationTicks", 0)));
                note.note =
                    juce::jlimit(0, 127, static_cast<int>(noteValue.getProperty("note", -1)));
                note.velocity =
                    juce::jlimit(1, 127, static_cast<int>(noteValue.getProperty("velocity", 0)));
                note.channel =
                    juce::jlimit(1, 16, static_cast<int>(noteValue.getProperty("channel", 0)));
                if (note.durationTicks == 0 || note.startTick >= midiClip.durationTicks) {
                    error = "Timeline MIDI note has an invalid musical range.";
                    return false;
                }
                midiClip.notes.push_back(note);
            }
            const auto events = value.getProperty("events", {});
            if (!events.isArray()) {
                error = "Timeline MIDI events must be an array.";
                return false;
            }
            for (const auto& eventValue : *events.getArray()) {
                if (!eventValue.isObject()) {
                    error = "Timeline MIDI event must be an object.";
                    return false;
                }
                MidiEvent event;
                event.kind = eventValue.getProperty("kind", {}).toString();
                event.tick = static_cast<std::uint64_t>(
                    static_cast<juce::int64>(eventValue.getProperty("tick", 0)));
                event.channel =
                    juce::jlimit(1, 16, static_cast<int>(eventValue.getProperty("channel", 0)));
                event.data1 =
                    juce::jlimit(0, 127, static_cast<int>(eventValue.getProperty("data1", 0)));
                event.data2 =
                    juce::jlimit(0, 127, static_cast<int>(eventValue.getProperty("data2", 0)));
                if (event.tick >= midiClip.durationTicks ||
                    (event.kind != "controlChange" && event.kind != "pitchBend" &&
                     event.kind != "channelPressure")) {
                    error = "Timeline MIDI event has an invalid type or musical position.";
                    return false;
                }
                midiClip.events.push_back(event);
            }
            track->midiClips.push_back(std::move(midiClip));
        }
        track->mixBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->liveInputBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->liveProcessedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->recordingCapture.processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }
    for (auto& track : prepared->tracks) {
        track->compensationDelaySamples =
            ArrangementGraph::compensationDelay(maximumPluginDelay, track->pluginDelaySamples);
        track->delayBuffer.setSize(
            2, static_cast<int>(track->compensationDelaySamples + maximumBlockSize + 1), false,
            true, false);
        track->delayBuffer.clear();
    }

    return true;
}

}  // namespace riffra
