#include "TimelineEngine.h"
#include "ArrangementGraph.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

namespace riffra {

namespace {
constexpr int kReadAheadSamples = 32768;

juce::String pluginTopologySignature(const juce::var& values) {
    juce::Array<juce::var> topology;
    const auto append = [&topology](const juce::var& value) {
        if (!value.isObject())
            return;
        auto* device = new juce::DynamicObject();
        device->setProperty("id", value.getProperty("id", {}));
        device->setProperty("kind", value.getProperty("kind", {}));
        device->setProperty("path", value.getProperty("path", {}));
        device->setProperty(
            "disabledPlaceholder",
            value.getProperty("disabledPlaceholder", false));
        topology.add(juce::var(device));
    };
    if (values.isArray()) {
        for (const auto& value : *values.getArray())
            append(value);
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

} // namespace

TimelineEngine::TimelineEngine(const bool offline)
    : offlineMode(offline),
      recordingCapture(std::make_unique<RecordingCaptureRuntime>()) {
    if (!offlineMode)
        readAheadThread.startThread();
}

TimelineEngine::~TimelineEngine() {
    stop();
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
        pendingTimeline.reset();
    }
    if (readAheadThread.isThreadRunning())
        readAheadThread.stopThread(3000);
}

bool TimelineEngine::loadSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    juce::String& error,
    const bool commitImmediately) {
    std::unique_ptr<PreparedTimeline> prepared;
    bool monitorLiveInputState = false;
    bool armedInstrumentTrackState = false;
    if (!prepareSnapshot(
            snapshot,
            formats,
            outputSampleRate,
            maximumBlockSize,
            prepared,
            monitorLiveInputState,
            armedInstrumentTrackState,
            error))
        return false;

    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        pendingTimeline = std::move(prepared);
        pendingMonitorLiveInput = monitorLiveInputState;
        pendingArmedInstrumentTrack = armedInstrumentTrackState;
    }
    if (!commitImmediately)
        return true;
    return commitPreparedSnapshot(error);
}

bool TimelineEngine::prepareSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    std::unique_ptr<PreparedTimeline>& prepared,
    bool& monitorLiveInputState,
    bool& armedInstrumentTrackState,
    juce::String& error) {
    prepared.reset();
    monitorLiveInputState = false;
    armedInstrumentTrackState = false;
    if (!snapshot.isObject() || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }
    prepared = std::make_unique<PreparedTimeline>();
    prepared->revision = static_cast<std::uint64_t>(
        static_cast<juce::int64>(snapshot.getProperty("revision", -1)));
    const auto unavailableClipIds = snapshot.getProperty("unavailableClipIds", {});
    if (unavailableClipIds.isArray())
        prepared->unavailableClipIds = *unavailableClipIds.getArray();
    const auto missingDeviceIds = snapshot.getProperty("missingDeviceIds", {});
    if (missingDeviceIds.isArray())
        prepared->missingDeviceIds = *missingDeviceIds.getArray();
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
        static_cast<std::uint64_t>(std::llround(beatTicks)),
        outputSampleRate);
    prepared->beatsPerBar = numerator;
    prepared->metronomeEnabled = static_cast<bool>(snapshot.getProperty("metronomeEnabled", false));

    const auto loopRange = snapshot.getProperty("loopRange", {});
    if (loopRange.isObject()) {
        prepared->loopEnabled = static_cast<bool>(loopRange.getProperty("enabled", false));
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("endTick", 0)));
        prepared->loopStartSample = prepared->timebase.tickToSample(
            startTick, outputSampleRate);
        prepared->loopEndSample = prepared->timebase.tickToSample(
            endTick, outputSampleRate);
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
        prepared->punchStartSample = prepared->timebase.tickToSample(
            startTick, outputSampleRate);
        prepared->punchEndSample = prepared->timebase.tickToSample(
            endTick, outputSampleRate);
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
        track->gainDb = juce::jlimit(
            -90.0f, 24.0f, static_cast<float>(trackValue.getProperty("gainDb", 0.0)));
        track->pan = juce::jlimit(
            -1.0f, 1.0f, static_cast<float>(trackValue.getProperty("pan", 0.0)));
        track->muted = static_cast<bool>(trackValue.getProperty("muted", false));
        track->solo = static_cast<bool>(trackValue.getProperty("solo", false));
        const auto monitoring = trackValue.getProperty("monitoring", {}).toString();
        track->monitorInput = ArrangementGraph::shouldMonitorAudioInput(
            monitoring, track->armed, track->instrument);
        if (track->monitorInput)
            monitorLiveInputState = true;
        const auto audioInput = trackValue.getProperty("audioInput", {});
        if (audioInput.isObject())
            track->audioInputChannel =
                static_cast<int>(audioInput.getProperty("channelIndex", -1));

        const auto automation = trackValue.getProperty(
            "automation", juce::var(juce::Array<juce::var> {}));
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
            auto& destination = parameter == "volume"
                ? track->volumeAutomation
                : track->panAutomation;
            for (const auto& pointValue : *pointValues.getArray()) {
                if (!pointValue.isObject()) {
                    error = "Timeline Automation Point must be an object.";
                    return false;
                }
                const auto tick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    pointValue.getProperty("tick", 0)));
                const auto value = static_cast<float>(pointValue.getProperty("value", 0.0));
                if (!std::isfinite(value)) {
                    error = "Timeline Automation Point must have a finite value.";
                    return false;
                }
                destination.push_back({
                    prepared->timebase.tickToSample(tick, outputSampleRate),
                    parameter == "volume"
                        ? juce::jlimit(-90.0f, 24.0f, value)
                        : juce::jlimit(-1.0f, 1.0f, value),
                });
            }
        }

        const auto rack = trackValue.getProperty("rack", {});
        const auto instrument = trackValue.getProperty("instrument", {});
        const auto devices = rack.isObject()
            ? rack.getProperty("devices", {})
            : juce::var(juce::Array<juce::var> {});
        track->effectTopologySignature = pluginTopologySignature(devices);
        track->instrumentTopologySignature = pluginTopologySignature(instrument);
        track->effectState = devices;
        track->instrumentState = instrument;
        track->instrumentDeviceId = instrument.isObject()
            ? instrument.getProperty("id", {}).toString()
            : juce::String();
        juce::var existingEffectState;
        juce::var existingInstrumentState;
        auto sameRuntimeTopology = false;
        {
            const juce::SpinLock::ScopedLockType lock(timelineLock);
            if (timeline != nullptr) {
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&track](const auto& item) { return item->id == track->id; });
                if (existing != timeline->tracks.end()
                    && (*existing)->effectTopologySignature
                        == track->effectTopologySignature
                    && (*existing)->instrumentTopologySignature
                        == track->instrumentTopologySignature) {
                    sameRuntimeTopology = true;
                    existingEffectState = (*existing)->effectState;
                    existingInstrumentState = (*existing)->instrumentState;
                    track->pluginDelaySamples = (*existing)->pluginDelaySamples;
                    track->pluginTailSamples = (*existing)->pluginTailSamples;
                }
            }
        }
        if (sameRuntimeTopology) {
            track->reuseRuntimeDevices =
                juce::JSON::toString(existingEffectState, false)
                    == juce::JSON::toString(track->effectState, false)
                && juce::JSON::toString(existingInstrumentState, false)
                    == juce::JSON::toString(track->instrumentState, false);
        }
        if (rack.isObject()) {
            if (!track->reuseRuntimeDevices
                && !track->effectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
            if (!track->reuseRuntimeDevices && !track->instrument &&
                !track->liveEffectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
            if (!track->reuseRuntimeDevices && !track->instrument &&
                !track->recordingCapture.effectChain.load(
                    devices, outputSampleRate, maximumBlockSize, error))
                return false;
        }
        if (instrument.isObject()
            && !static_cast<bool>(
                instrument.getProperty("disabledPlaceholder", false))
            && !track->reuseRuntimeDevices) {
            const auto path = instrument.getProperty("path", {}).toString();
            track->instrumentRack = std::make_unique<PluginRack>();
            if (const auto loadError =
                    track->instrumentRack->load(path, outputSampleRate, maximumBlockSize)) {
                error = "Track Instrument could not be loaded: " + loadError->message;
                return false;
            }
            if (!track->instrumentRack->applyPersistedState(instrument, error))
                return false;
        }
        if (!track->reuseRuntimeDevices) {
            track->pluginDelaySamples = track->effectChain.latencySamples() +
                (track->instrumentRack != nullptr ? track->instrumentRack->latencySamples() : 0);
            track->pluginTailSamples = track->effectChain.tailSamples()
                + (track->instrumentRack != nullptr ? track->instrumentRack->tailSamples() : 0);
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
            auto reader = std::unique_ptr<juce::AudioFormatReader>(
                formats.createReaderFor(juce::File(path)));
            if (reader == nullptr || reader->lengthInSamples <= 0 || reader->sampleRate <= 0.0) {
                error = "Timeline source could not be opened: " + path;
                return false;
            }
            auto clip = std::make_unique<Clip>();
            clip->id = value.getProperty("clipId", {}).toString();
            const auto declaredSourceRate = static_cast<double>(
                value.getProperty("sourceSampleRate", 0.0));
            clip->sourceSampleRate = reader->sampleRate;
            clip->sourceStartFrame = static_cast<juce::int64>(
                value.getProperty("sourceStartFrame", 0));
            clip->sourceEndFrame = static_cast<juce::int64>(
                value.getProperty("sourceEndFrame", 0));
            const auto durationFrames = static_cast<juce::int64>(
                value.getProperty("durationFrames", 0));
            const auto durationRate = static_cast<double>(
                value.getProperty("durationSampleRate", 0.0));
            if (clip->id.isEmpty() || declaredSourceRate <= 0.0 ||
                std::abs(declaredSourceRate - reader->sampleRate) > 0.5 ||
                clip->sourceStartFrame < 0 ||
                clip->sourceEndFrame <= clip->sourceStartFrame ||
                clip->sourceEndFrame > reader->lengthInSamples || durationFrames <= 0 ||
                durationRate <= 0.0) {
                error = "Timeline clip has an invalid frame range: " + clip->id;
                return false;
            }
            const auto startTick = static_cast<std::uint64_t>(
                static_cast<juce::int64>(value.getProperty("startTick", 0)));
            clip->startSample = prepared->timebase.tickToSample(
                startTick, outputSampleRate);
            clip->durationSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(durationFrames) * outputSampleRate / durationRate));
            const auto fadeInFrames = static_cast<juce::int64>(
                value.getProperty("fadeInFrames", 0));
            const auto fadeOutFrames = static_cast<juce::int64>(
                value.getProperty("fadeOutFrames", 0));
            clip->fadeInSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(fadeInFrames) * outputSampleRate / durationRate));
            clip->fadeOutSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(fadeOutFrames) * outputSampleRate / durationRate));
            clip->gain = juce::Decibels::decibelsToGain(
                static_cast<float>(value.getProperty("gainDb", 0.0)));
            clip->pan = juce::jlimit(
                -1.0f, 1.0f, static_cast<float>(value.getProperty("pan", 0.0)));
            clip->loop = static_cast<bool>(value.getProperty("loopEnabled", false));
            clip->muted = static_cast<bool>(value.getProperty("muted", false));
            clip->readerSource = std::make_unique<juce::AudioFormatReaderSource>(reader.release(), true);
            clip->positionableSource = clip->readerSource.get();
            if (!offlineMode) {
                clip->bufferingSource = std::make_unique<juce::BufferingAudioSource>(
                    clip->readerSource.get(),
                    readAheadThread,
                    false,
                    kReadAheadSamples,
                    2);
                clip->positionableSource = clip->bufferingSource.get();
            }
            clip->resamplingSource = std::make_unique<juce::ResamplingAudioSource>(
                clip->positionableSource,
                false,
                2);
            clip->resamplingSource->setResamplingRatio(
                clip->sourceSampleRate / outputSampleRate);
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
            midiClip.startTick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                value.getProperty("startTick", 0)));
            midiClip.durationTicks = static_cast<std::uint64_t>(static_cast<juce::int64>(
                value.getProperty("durationTicks", 0)));
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
                note.startTick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    noteValue.getProperty("startTick", 0)));
                note.durationTicks = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    noteValue.getProperty("durationTicks", 0)));
                note.note = juce::jlimit(0, 127, static_cast<int>(noteValue.getProperty("note", -1)));
                note.velocity = juce::jlimit(
                    1, 127, static_cast<int>(noteValue.getProperty("velocity", 0)));
                note.channel = juce::jlimit(
                    1, 16, static_cast<int>(noteValue.getProperty("channel", 0)));
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
                event.tick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    eventValue.getProperty("tick", 0)));
                event.channel = juce::jlimit(
                    1, 16, static_cast<int>(eventValue.getProperty("channel", 0)));
                event.data1 = juce::jlimit(0, 127, static_cast<int>(
                    eventValue.getProperty("data1", 0)));
                event.data2 = juce::jlimit(0, 127, static_cast<int>(
                    eventValue.getProperty("data2", 0)));
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
        track->recordingCapture.processedBuffer.setSize(
            2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }
    for (auto& track : prepared->tracks) {
        track->compensationDelaySamples = ArrangementGraph::compensationDelay(
            maximumPluginDelay, track->pluginDelaySamples);
        track->delayBuffer.setSize(
            2, static_cast<int>(track->compensationDelaySamples + maximumBlockSize + 1),
            false, true, false);
        track->delayBuffer.clear();
    }

    return true;
}

bool TimelineEngine::commitPreparedSnapshot(juce::String& error) noexcept {
    std::unique_ptr<PreparedTimeline> candidate;
    std::unique_ptr<PreparedTimeline> retiredTimeline;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        if (pendingTimeline == nullptr) {
            error = "No prepared Timeline snapshot is available.";
            return false;
        }
        candidate = std::move(pendingTimeline);
        if (timeline != nullptr) {
            // Validate every reusable runtime before moving ownership. The
            // prepared graph was built against the active graph, but a direct
            // native mutation may have changed the topology in the meantime.
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices)
                    continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id
                            && item->effectTopologySignature
                                == candidateTrack->effectTopologySignature
                            && item->instrumentTopologySignature
                                == candidateTrack->instrumentTopologySignature;
                    });
                if (existing == timeline->tracks.end()) {
                    error = "Timeline device runtime changed while the snapshot was prepared.";
                    pendingTimeline = std::move(candidate);
                    return false;
                }
            }
            // State application already happened while the candidate was
            // prepared. Publishing now only transfers reusable ownership and
            // swaps the graph pointer; no VST lifecycle method runs under this
            // lock.
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices)
                    continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id
                            && item->effectTopologySignature
                                == candidateTrack->effectTopologySignature
                            && item->instrumentTopologySignature
                                == candidateTrack->instrumentTopologySignature;
                    });
                if (existing == timeline->tracks.end())
                    continue;
                (*existing)->effectChain = std::move(candidateTrack->effectChain);
                (*existing)->liveEffectChain = std::move(candidateTrack->liveEffectChain);
                (*existing)->recordingCapture.effectChain =
                    std::move(candidateTrack->recordingCapture.effectChain);
                (*existing)->instrumentRack = std::move(candidateTrack->instrumentRack);
            }
        }

        retiredTimeline = std::move(timeline);
        timeline = std::move(candidate);
        monitorLiveInput.store(pendingMonitorLiveInput, std::memory_order_release);
        armedInstrumentTrack.store(pendingArmedInstrumentTrack, std::memory_order_release);
        if (retiredTimeline == nullptr)
            timelineSample.store(0, std::memory_order_release);
        discontinuity.fetch_add(1, std::memory_order_relaxed);
        sequence.fetch_add(1, std::memory_order_relaxed);
    }
    return true;
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    pendingTimeline.reset();
}

void TimelineEngine::play() noexcept {
    state.store(State::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(State::stopped, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        resetPlaybackTrackState(*timeline);
        resetRecordingTrackState(*timeline);
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        resetPlaybackTrackState(*timeline);
        resetRecordingTrackState(*timeline);
    }
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    timelineSample.store(
        timeline->timebase.tickToSample(tick, timeline->outputSampleRate),
        std::memory_order_release);
    resetPlaybackTrackState(*timeline);
    resetRecordingTrackState(*timeline);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::startRecording(const int countInBeats, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr || timeline->outputSampleRate <= 0.0) {
        error = "Arrange recording requires a prepared Arrangement Graph.";
        return false;
    }
    if (recordingPhase.load(std::memory_order_acquire) != RecordingPhase::idle) {
        error = "Arrange recording is already active.";
        return false;
    }
    for (auto& track : timeline->tracks) {
        recordingCapture->resetTrack(track->recordingCapture);
        if (!track->instrument)
            track->recordingCapture.effectChain.reset();
    }
    recordingCapture->resetDrainingTailTracks();
    recordingCapture->resetCaptureErrors();
    recordingPassOrdinal.store(1, std::memory_order_release);
    const auto alreadyPlaying =
        state.load(std::memory_order_acquire) == State::playing;
    if (alreadyPlaying || countInBeats <= 0) {
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        recordingStartAudioSample.store(
            audioClockSample.load(std::memory_order_acquire), std::memory_order_release);
        const auto tick = timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire),
            timeline->outputSampleRate);
        recordingStartTick.store(tick, std::memory_order_release);
        if (!alreadyPlaying)
            state.store(State::playing, std::memory_order_release);
    } else {
        countInRemainingSamples.store(
            timeline->beatSamples * std::max(0, countInBeats), std::memory_order_release);
        recordingPhase.store(RecordingPhase::countingIn, std::memory_order_release);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::stopRecording() noexcept {
    recordingPhase.store(RecordingPhase::stopping, std::memory_order_release);
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    const auto hasCaptureWork = timeline != nullptr
        && std::any_of(timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
               return recordingCapture->hasCaptureWork(track->recordingCapture);
           });
    if (!hasCaptureWork)
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::cancelRecordingIfCountingIn() noexcept {
    auto expected = RecordingPhase::countingIn;
    if (!recordingPhase.compare_exchange_strong(
            expected,
            RecordingPhase::idle,
            std::memory_order_acq_rel))
        return false;
    countInRemainingSamples.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::flushRecordingTail(juce::String& error) noexcept {
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        auto sinkLease = recordingCapture->acquireSink();
        auto* sink = sinkLease.get();
        if (timeline == nullptr || sink == nullptr) {
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return true;
        }
        if (timeline->loopEnabled) {
            // Loop recording: close any active segment, then generate processed offline
            for (auto& trackPtr : timeline->tracks) {
                auto& track = *trackPtr;
                if (!track.armed || track.instrument
                    || track.recordingCapture.state != RecordingCaptureState::capturing)
                    continue;
                (void) recordingCapture->endTrackCapture(
                    track.id, track.recordingCapture);
                track.recordingCapture.state = RecordingCaptureState::idle;
            }
            if (!generateLoopProcessedVariants(*timeline, sink)) {
                error = "Loop recording Processed Variant generation failed.";
                return false;
            }
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return recordingCapture->captureErrors() == 0;
        }
        for (auto& trackPtr : timeline->tracks) {
            auto& track = *trackPtr;
            if (!track.armed || track.instrument
                || track.recordingCapture.state != RecordingCaptureState::capturing)
                continue;
            if (!recordingCapture->beginTailDrain(
                    track.id,
                    track.recordingCapture,
                    track.pluginDelaySamples,
                    track.pluginTailSamples)) {
                error = "Recording Capture Segment could not be closed for tail drain.";
                return false;
            }
        }
    }

    const auto deadline = juce::Time::getMillisecondCounter() + 5000u;
    while (recordingCapture->drainingTailTracks() != 0) {
        if (juce::Time::getMillisecondCounter() >= deadline) {
            error = "Processed recording tail did not drain before the realtime deadline.";
            return false;
        }
        juce::Thread::sleep(1);
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    return recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::generateLoopProcessedVariants(
    PreparedTimeline& prepared,
    ArrangementCaptureSink* const sink) noexcept {
    const auto blockSize = std::max(1, prepared.preparedBlockSize);
    juce::AudioFormatManager formatReader;
    formatReader.registerBasicFormats();
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.instrument || !track.armed)
            continue;
        const auto rawFile = sink->prepareRawForReading(track.id);
        if (rawFile == juce::File {})
            continue;
        const auto segments = sink->getRawSegmentRanges(track.id);
        if (segments.empty())
            continue;
        // Open the flushed raw file as a stream so that non-.wav extensions
        // (e.g. .partial) are accepted by the AudioFormatManager readers.
        auto rawStream = rawFile.createInputStream();
        if (rawStream == nullptr || !rawStream->openedOk())
            return false;
        std::unique_ptr<juce::AudioFormatReader> reader(
            formatReader.createReaderFor(std::move(rawStream)));
        if (reader == nullptr)
            return false;
        const auto delay = static_cast<int>(std::max<std::int64_t>(
            0, track.pluginDelaySamples));
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentSamples = static_cast<int>(segEnd - segStart);
            if (segmentSamples <= 0)
                continue;
            track.recordingCapture.effectChain.reset();
            // Output buffer holds segmentSamples + delay for latency alignment
            const auto outputSize = segmentSamples + delay;
            juce::AudioBuffer<float> outputBuffer(2, outputSize);
            outputBuffer.clear();
            int outputOffset = 0;
            // Process raw segment in block-sized chunks
            juce::AudioBuffer<float> blockBuffer(2, blockSize);
            int remaining = segmentSamples;
            std::int64_t readPos = static_cast<std::int64_t>(segStart);
            while (remaining > 0) {
                const auto count = std::min(blockSize, remaining);
                blockBuffer.clear();
                reader->read(
                    blockBuffer.getArrayOfWritePointers(), 2,
                    readPos, count);
                readPos += count;
                const std::array<float*, 2> outPtrs {
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(
                    blockBuffer.getArrayOfReadPointers(),
                    2,
                    outPtrs.data(),
                    2,
                    count);
                outputOffset += count;
                remaining -= count;
            }
            // Feed delay zeros to flush plugin latency
            int delayRemaining = delay;
            while (delayRemaining > 0) {
                const auto count = std::min(blockSize, delayRemaining);
                blockBuffer.clear();
                const std::array<float*, 2> outPtrs {
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(
                    blockBuffer.getArrayOfReadPointers(),
                    2,
                    outPtrs.data(),
                    2,
                    count);
                outputOffset += count;
                delayRemaining -= count;
            }
            // Discard first delay samples, write segmentSamples in block-sized chunks
            constexpr int kOfflineWriterTimeoutMs = 5000;
            int writeOffset = 0;
            while (writeOffset < segmentSamples) {
                const auto count = std::min(
                    blockSize,
                    segmentSamples - writeOffset);
                const std::array<const float*, 2> processedBlock {
                    outputBuffer.getReadPointer(0) + delay + writeOffset,
                    outputBuffer.getReadPointer(1) + delay + writeOffset,
                };
                if (!sink->writeProcessedAudioTrackOffline(
                        track.id,
                        processedBlock.data(),
                        count,
                        kOfflineWriterTimeoutMs)) {
                    return false;
                }
                writeOffset += count;
            }
        }
    }
    return true;
}

juce::var TimelineEngine::recordingConfiguration() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr)
        return {};
    auto* result = new juce::DynamicObject();
    result->setProperty("sampleRate", timeline->outputSampleRate);
    const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
        timelineSample.load(std::memory_order_acquire),
        timeline->outputSampleRate));
    result->setProperty("timelineStartTick", tick);
    result->setProperty("loopEnabled", timeline->loopEnabled);
    result->setProperty("loopStartSample", static_cast<juce::int64>(timeline->loopStartSample));
    result->setProperty("loopEndSample", static_cast<juce::int64>(timeline->loopEndSample));
    result->setProperty("punchEnabled", timeline->punchEnabled);
    result->setProperty("punchStartSample", static_cast<juce::int64>(timeline->punchStartSample));
    result->setProperty("punchEndSample", static_cast<juce::int64>(timeline->punchEndSample));
    juce::Array<juce::var> trackValues;
    for (const auto& track : timeline->tracks) {
        if (!track->armed)
            continue;
        auto* value = new juce::DynamicObject();
        value->setProperty("trackId", track->id);
        value->setProperty("kind", track->instrument ? "instrument" : "audio");
        value->setProperty("audioInputChannel", track->audioInputChannel);
        value->setProperty("midiDeviceId", track->midiDeviceId);
        value->setProperty("midiChannel", track->midiChannel);
        value->setProperty(
            "pluginLatencySamples", static_cast<int>(track->pluginDelaySamples));
        value->setProperty(
            "pluginTailSamples", static_cast<int>(track->pluginTailSamples));
        trackValues.add(juce::var(value));
    }
    result->setProperty("tracks", trackValues);
    return juce::var(result);
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingCapture->setSink(sink);
}

void TimelineEngine::clearRecordingSink() noexcept {
    recordingCapture->clearSink();
}

bool TimelineEngine::enqueueLiveMidi(
    const juce::MidiMessage& message,
    const juce::String& deviceId) noexcept {
    if (!armedInstrumentTrack.load(std::memory_order_acquire))
        return false;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr)
        return true;
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (track.instrument && track.armed
            && ArrangementGraph::midiRouteMatches(
                track.midiDeviceId, track.midiChannel, deviceId, message.getChannel())) {
            if (track.instrumentRack != nullptr)
                track.instrumentRack->enqueueMidi(message);
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(
                    track.id,
                    deviceId,
                    message,
                    audioClockSample.load(std::memory_order_acquire));
            }
        }
    }
    return true;
}

PluginRack* TimelineEngine::findDevice(
    const juce::String& trackId,
    const juce::String& deviceId) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr)
        return nullptr;
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end())
        return nullptr;
    auto& track = **found;
    const auto instrument = track.instrumentRack.get();
    if (instrument != nullptr && deviceId == track.instrumentDeviceId)
        return instrument;
    return track.effectChain.findDevice(deviceId);
}

bool TimelineEngine::mirrorEditorDeviceState(
    const juce::String& trackId,
    const juce::String& deviceId,
    const juce::var& persistedState,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return true;
    auto* live = track.liveEffectChain.findDevice(deviceId);
    if (live == nullptr) {
        error = "Live Track Device was not found.";
        return false;
    }
    if (!live->applyPersistedState(persistedState, error))
        return false;
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (recording != nullptr && !recording->applyPersistedState(persistedState, error))
        return false;
    return true;
}

bool TimelineEngine::mirrorEditorDeviceParameter(
    const juce::String& trackId,
    const juce::String& deviceId,
    const int parameterIndex,
    const float value,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return true;
    auto* live = track.liveEffectChain.findDevice(deviceId);
    if (live == nullptr) {
        error = "Live Track Device was not found.";
        return false;
    }
    live->enqueueParameterChange(parameterIndex, value);
    if (auto* recording = track.recordingCapture.effectChain.findDevice(deviceId))
        recording->enqueueParameterChange(parameterIndex, value);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

juce::var TimelineEngine::devicePersistedState(
    const juce::String& trackId,
    const juce::String& deviceId,
    juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return {};
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return track.instrumentRack->persistedState(error);
    return track.effectChain.persistedState(deviceId, error);
}

bool TimelineEngine::preparedTrackReusesRuntimeDevices(
    const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (pendingTimeline == nullptr)
        return false;
    const auto track = std::find_if(
        pendingTimeline->tracks.begin(),
        pendingTimeline->tracks.end(),
        [&trackId](const auto& item) { return item->id == trackId; });
    return track != pendingTimeline->tracks.end()
        && (*track)->reuseRuntimeDevices;
}

bool TimelineEngine::hasPreparedSnapshot() const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return pendingTimeline != nullptr;
}

bool TimelineEngine::setDeviceBypassed(
    const juce::String& trackId,
    const juce::String& deviceId,
    const bool bypassed,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId) {
        track.instrumentRack->setBypassed(bypassed);
    } else {
        auto* playback = track.effectChain.findDevice(deviceId);
        auto* live = track.liveEffectChain.findDevice(deviceId);
        auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
        if (playback == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        playback->setBypassed(bypassed);
        if (live != nullptr)
            live->setBypassed(bypassed);
        if (recording != nullptr)
            recording->setBypassed(bypassed);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceParameter(
    const juce::String& trackId,
    const juce::String& deviceId,
    const int parameterIndex,
    const float value,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    auto* playback = track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId
        ? track.instrumentRack.get()
        : track.effectChain.findDevice(deviceId);
    auto* live = track.liveEffectChain.findDevice(deviceId);
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (playback == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    const auto parameterStatus = playback->parameterStatus().getProperty("parameters", {});
    if (!parameterStatus.isArray() || parameterIndex < 0 || parameterIndex >= parameterStatus.size()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    const auto previous =
        static_cast<float>(parameterStatus[parameterIndex].getProperty("value", 0.0));
    if (!playback->setParameter(parameterIndex, value, error))
        return false;
    if (live != nullptr && !live->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void) playback->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    if (recording != nullptr && !recording->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void) playback->setParameter(parameterIndex, previous, rollbackError);
        if (live != nullptr)
            (void) live->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return monitorLiveInput.load(std::memory_order_acquire);
}

bool TimelineEngine::recordingWindow(
    const int sampleCount,
    int& sampleOffset,
    int& capturedSamples) noexcept {
    sampleOffset = 0;
    capturedSamples = std::max(0, sampleCount);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    if (sampleCount <= 0)
        return false;
    auto phase = recordingPhase.load(std::memory_order_acquire);
    auto transitionedFromCountIn = false;
    if (phase == RecordingPhase::idle || phase == RecordingPhase::stopping) {
        capturedSamples = 0;
        return false;
    }
    if (phase == RecordingPhase::countingIn) {
        const auto remaining = countInRemainingSamples.load(std::memory_order_acquire);
        countInBlockStartRemainingSamples.store(remaining, std::memory_order_release);
        if (remaining >= sampleCount) {
            countInRemainingSamples.store(
                remaining - sampleCount, std::memory_order_release);
            capturedSamples = 0;
            return false;
        }
        sampleOffset = static_cast<int>(std::max<std::int64_t>(0, remaining));
        playbackBlockOffset.store(sampleOffset, std::memory_order_release);
        capturedSamples = sampleCount - sampleOffset;
        countInRemainingSamples.store(0, std::memory_order_release);
        recordingStartAudioSample.store(
            audioClockSample.load(std::memory_order_acquire) +
                static_cast<std::uint64_t>(sampleOffset),
            std::memory_order_release);
        const juce::SpinLock::ScopedTryLockType lock(timelineLock);
        if (lock.isLocked() && timeline != nullptr) {
            const auto tick = timeline->timebase.sampleToTick(
                timelineSample.load(std::memory_order_acquire),
                timeline->outputSampleRate);
            recordingStartTick.store(tick, std::memory_order_release);
        }
        state.store(State::playing, std::memory_order_release);
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        phase = RecordingPhase::recording;
        transitionedFromCountIn = true;
    }

    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->punchEnabled) {
        captureBlockOffset.store(sampleOffset, std::memory_order_release);
        captureBlockSamples.store(capturedSamples, std::memory_order_release);
        return true;
    }

    const auto position = timelineSample.load(std::memory_order_acquire);
    const auto playbackOffset = transitionedFromCountIn ? sampleOffset : 0;
    const auto playbackSamples = sampleCount - playbackOffset;
    const auto blockEnd = position + static_cast<std::int64_t>(playbackSamples);
    if (blockEnd <= timeline->punchStartSample || position >= timeline->punchEndSample) {
        capturedSamples = 0;
        return false;
    }
    const auto punchOffset = static_cast<int>(std::max<std::int64_t>(
        0, timeline->punchStartSample - position));
    sampleOffset = playbackOffset + punchOffset;
    const auto end = std::min<std::int64_t>(blockEnd, timeline->punchEndSample);
    capturedSamples = static_cast<int>(std::max<std::int64_t>(
        0, end - position - punchOffset));
    captureBlockOffset.store(sampleOffset, std::memory_order_release);
    captureBlockSamples.store(capturedSamples, std::memory_order_release);
    return capturedSamples > 0;
}

void TimelineEngine::mixMetronome(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    if (sampleCount <= 0)
        return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->metronomeEnabled
        || timeline->beatSamples <= 0)
        return;
    const auto loopLength = timeline->loopEndSample - timeline->loopStartSample;
    const auto start = lastMixStartSample.load(std::memory_order_acquire);
    const auto playbackOffset = juce::jlimit(
        0, sampleCount, lastMixPlaybackOffset.load(std::memory_order_acquire));
    const auto countInRemaining =
        countInBlockStartRemainingSamples.load(std::memory_order_acquire);
    const auto countingIn =
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::countingIn;
    const auto playing = state.load(std::memory_order_acquire) == State::playing;
    constexpr std::int64_t clickSamples = 1'920;
    for (int sample = 0; sample < sampleCount; ++sample) {
        float value = 0.0f;
        if (countInRemaining > 0
            && sample < (countingIn ? sampleCount : playbackOffset)) {
            const auto remaining = countInRemaining - sample;
            const auto offset = (timeline->beatSamples
                - remaining % timeline->beatSamples) % timeline->beatSamples;
            if (offset >= 0 && offset < clickSamples) {
                const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                value = 0.11f * envelope;
            }
        } else if (playing && sample >= playbackOffset) {
            auto position = start + sample - playbackOffset;
            if (timeline->loopEnabled && loopLength > 0
                && position >= timeline->loopEndSample)
                position = timeline->loopStartSample +
                    (position - timeline->loopEndSample) % loopLength;
            if (position >= 0) {
                const auto beat = position / timeline->beatSamples;
                const auto offset = position % timeline->beatSamples;
                if (offset >= 0 && offset < clickSamples) {
                    const auto envelope =
                        1.0f - static_cast<float>(offset) / clickSamples;
                    const auto amplitude =
                        beat % timeline->beatsPerBar == 0 ? 0.18f : 0.11f;
                    value = amplitude * envelope;
                }
            }
        }
        if (value <= 0.0f)
            continue;
        for (int channel = 0; channel < channelCount; ++channel) {
            if (outputChannels[channel] != nullptr)
                outputChannels[channel][sample] += value;
        }
    }
}

void TimelineEngine::mixRange(
    Track& track,
    const std::int64_t rangeStart,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : track.clips) {
        auto& clip = *clipPtr;
        if (clip.muted) continue;
        const auto clipEnd = clip.startSample + clip.durationSamples;
        const auto overlapStart = std::max(rangeStart, clip.startSample);
        const auto overlapEnd = std::min(rangeEnd, clipEnd);
        if (overlapEnd <= overlapStart) continue;
        auto remaining = static_cast<int>(overlapEnd - overlapStart);
        auto outputOffset = destinationStart + static_cast<int>(overlapStart - rangeStart);
        auto localSample = overlapStart - clip.startSample;
        while (remaining > 0) {
            const auto sourceRange = clip.sourceEndFrame - clip.sourceStartFrame;
            auto sourceOffset = static_cast<std::int64_t>(std::floor(
                static_cast<double>(localSample) * clip.sourceSampleRate /
                track.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd = static_cast<int>(std::ceil(
                static_cast<double>(sourceRemaining) * track.outputSampleRate /
                clip.sourceSampleRate));
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
                    const auto progress = static_cast<float>(position) /
                        static_cast<float>(clip.fadeInSamples);
                    envelope = std::min(
                        envelope,
                        std::sin(juce::MathConstants<float>::halfPi * progress));
                }
                const auto remainingClip = clip.durationSamples - position - 1;
                if (clip.fadeOutSamples > 0 && remainingClip < clip.fadeOutSamples) {
                    const auto progress = static_cast<float>(
                        std::max<std::int64_t>(0, remainingClip)) /
                        static_cast<float>(clip.fadeOutSamples);
                    envelope = std::min(
                        envelope,
                        std::sin(juce::MathConstants<float>::halfPi * progress));
                }
                const auto panAngle = (clip.pan + 1.0f) *
                    juce::MathConstants<float>::pi * 0.25f;
                const auto source = clip.scratch.getSample(0, sample) * clip.gain * envelope;
                track.mixBuffer.addSample(
                    0, outputOffset + sample, source * std::cos(panAngle));
                track.mixBuffer.addSample(
                    1, outputOffset + sample,
                    clip.scratch.getNumChannels() > 1
                        ? clip.scratch.getSample(1, sample) * clip.gain * envelope * std::sin(panAngle)
                        : source * std::sin(panAngle));
            }
            clip.expectedSourceFrame = sourceFrame + static_cast<std::int64_t>(std::floor(
                static_cast<double>(chunk) * clip.sourceSampleRate /
                track.outputSampleRate));
            remaining -= chunk;
            outputOffset += chunk;
            localSample += chunk;
            if (!clip.loop && sourceFrame + sourceRemaining >= clip.sourceEndFrame && remaining > 0)
                break;
            if (clip.loop && remaining > 0) clip.expectedSourceFrame = -1;
        }
    }
}

void TimelineEngine::scheduleMidi(
    const PreparedTimeline& prepared,
    Track& track,
    const std::int64_t rangeStart,
    const int sampleCount) noexcept {
    track.midiBuffer.clear();
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : track.midiClips) {
        if (clip.muted) continue;
        const auto clipStart = prepared.timebase.tickToSample(
            clip.startTick, track.outputSampleRate);
        const auto clipLength = std::max<std::int64_t>(1, prepared.timebase.tickToSample(
            clip.durationTicks, track.outputSampleRate));
        const auto firstIteration = clip.loop && rangeStart > clipStart
            ? std::max<std::int64_t>(0, (rangeStart - clipStart) / clipLength - 1)
            : 0;
        const auto lastIteration = clip.loop
            ? std::max<std::int64_t>(firstIteration,
                (rangeEnd - clipStart) / clipLength + 1)
            : 0;
        const auto addMessage = [&](const juce::MidiMessage& message, const std::int64_t sample) {
            if (sample >= rangeStart && sample < rangeEnd)
                track.midiBuffer.addEvent(
                    message, juce::jlimit(0, sampleCount - 1,
                        static_cast<int>(sample - rangeStart)));
        };
        for (std::int64_t iteration = firstIteration; iteration <= lastIteration; ++iteration) {
            const auto iterationStart = clipStart + iteration * clipLength;
            for (const auto& note : clip.notes) {
                const auto noteStart = iterationStart + prepared.timebase.tickToSample(
                    note.startTick, track.outputSampleRate);
                const auto noteEnd = std::min(
                    iterationStart + clipLength,
                    noteStart + std::max<std::int64_t>(1, prepared.timebase.tickToSample(
                        note.durationTicks, track.outputSampleRate)));
                addMessage(juce::MidiMessage::noteOn(
                    juce::jlimit(1, 16, note.channel), note.note,
                    static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))), noteStart);
                addMessage(juce::MidiMessage::noteOff(
                    juce::jlimit(1, 16, note.channel), note.note), noteEnd);
            }
            for (const auto& event : clip.events) {
                const auto eventSample = iterationStart + prepared.timebase.tickToSample(
                    event.tick, track.outputSampleRate);
                const auto channel = juce::jlimit(1, 16, event.channel);
                if (event.kind == "controlChange")
                    addMessage(juce::MidiMessage::controllerEvent(
                        channel, event.data1, event.data2), eventSample);
                else if (event.kind == "pitchBend")
                    addMessage(juce::MidiMessage::pitchWheel(
                        channel, event.data1 | (event.data2 << 7)), eventSample);
                else if (event.kind == "channelPressure")
                    addMessage(juce::MidiMessage::channelPressureChange(
                        channel, event.data1), eventSample);
            }
            if (!clip.loop) break;
        }
    }
}

void TimelineEngine::processTracks(
    PreparedTimeline& prepared,
    const float* const* physicalInputChannels,
    const int physicalInputChannelCount,
    float* const* outputChannels,
    const int channelCount,
    const std::int64_t rangeStart,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(
        prepared.tracks.begin(), prepared.tracks.end(),
        [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        const auto audible = !track.muted && (!hasSolo || track.solo);
        track.processedBuffer.clear(0, sampleCount);
        const float* inputChannels[2] = {
            track.mixBuffer.getWritePointer(0), track.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {
            track.processedBuffer.getWritePointer(0), track.processedBuffer.getWritePointer(1)};
        if (track.instrument) {
            if (track.instrumentRack != nullptr)
                track.instrumentRack->process(
                    nullptr, 0, track.mixBuffer.getArrayOfWritePointers(), 2, sampleCount,
                    &track.midiBuffer);
            else
                track.mixBuffer.clear(0, sampleCount);
            track.effectChain.process(
                track.mixBuffer.getArrayOfReadPointers(),
                2,
                processedChannels,
                2,
                sampleCount);
        } else {
            track.effectChain.process(
                inputChannels, 2, processedChannels, 2, sampleCount);
        }
        const auto delay = track.compensationDelaySamples;
        const auto delaySize = track.delayBuffer.getNumSamples();
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto timelinePosition = rangeStart + sample;
            const auto gain = juce::Decibels::decibelsToGain(
                ArrangementGraph::automationValueAt(
                    track.volumeAutomation, timelinePosition, track.gainDb));
            const auto pan = juce::jlimit(
                -1.0f,
                1.0f,
                ArrangementGraph::automationValueAt(
                    track.panAutomation, timelinePosition, track.pan));
            const auto panAngle =
                (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
            const auto leftGain = gain * std::cos(panAngle);
            const auto rightGain = gain * std::sin(panAngle);
            float left = processedChannels[0][sample];
            float right = processedChannels[1][sample];
            if (delay > 0 && delaySize > 0) {
                const auto write = track.delayWritePosition;
                track.delayBuffer.setSample(0, static_cast<int>(write), left);
                track.delayBuffer.setSample(1, static_cast<int>(write), right);
                const auto read = (write - delay + delaySize) % delaySize;
                left = track.delayBuffer.getSample(0, static_cast<int>(read));
                right = track.delayBuffer.getSample(1, static_cast<int>(read));
                track.delayWritePosition = (write + 1) % delaySize;
            }
            if (audible && channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + sample] += left * leftGain;
            if (audible && channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + sample] += right * rightGain;
        }
        if (!track.instrument && (track.monitorInput || track.armed)
            && track.audioInputChannel >= 0) {
            const auto* source = ArrangementGraph::audioInputSource(
                track.audioInputChannel,
                physicalInputChannels,
                physicalInputChannelCount);
            for (int channel = 0; channel < 2; ++channel) {
                auto* destination = track.liveInputBuffer.getWritePointer(channel);
                if (source != nullptr)
                    juce::FloatVectorOperations::copy(
                        destination, source + destinationStart, sampleCount);
                else
                    juce::FloatVectorOperations::clear(destination, sampleCount);
            }
            track.liveEffectChain.process(
                track.liveInputBuffer.getArrayOfReadPointers(),
                2,
                track.liveProcessedBuffer.getArrayOfWritePointers(),
                2,
                sampleCount);
            const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
            const auto captureEnd =
                captureStart + captureBlockSamples.load(std::memory_order_acquire);
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (track.armed) {
                auto& capture = track.recordingCapture;
                if (writeEnd > writeStart) {
                    const auto localOffset = writeStart - destinationStart;
                    const auto captureAudioStart = callbackAudioStartSample.load(
                        std::memory_order_acquire) + static_cast<std::uint64_t>(writeStart);
                    const auto captureTimelineStart = static_cast<std::uint64_t>(
                        rangeStart + localOffset);
                    const auto discontinuous =
                        capture.state != RecordingCaptureState::capturing
                        || captureAudioStart != capture.endAudioSample;
                    if (discontinuous
                        && capture.state == RecordingCaptureState::capturing) {
                        if (!recordingCapture->beginTailDrain(
                                track.id,
                                capture,
                                track.pluginDelaySamples,
                                track.pluginTailSamples))
                            capture.state = RecordingCaptureState::completed;
                    }
                    if (capture.state == RecordingCaptureState::idle) {
                        capture.effectChain.reset();
                        capture.latencyToDiscard = static_cast<int>(std::max<std::int64_t>(
                            0, track.pluginDelaySamples));
                        (void) recordingCapture->beginTrackCapture(
                            track.id,
                            capture,
                            captureAudioStart,
                            captureTimelineStart);
                    }
                    if (capture.state == RecordingCaptureState::capturing) {
                        const auto writeCount = writeEnd - writeStart;
                        const auto* rawPointer =
                            track.liveInputBuffer.getReadPointer(0) + localOffset;
                        if (prepared.loopEnabled) {
                            // Loop recording: write raw only (processed generated offline)
                            recordingCapture->writeAudioTrack(
                                track.id, rawPointer, writeCount, nullptr, 0);
                        } else {
                            // Normal recording: write raw + processed in real-time
                            capture.processedBuffer.clear(0, writeCount);
                            const std::array<const float*, 2> recordingInput {
                                track.liveInputBuffer.getReadPointer(0) + localOffset,
                                track.liveInputBuffer.getReadPointer(1) + localOffset,
                            };
                            capture.effectChain.process(
                                recordingInput.data(),
                                2,
                                capture.processedBuffer.getArrayOfWritePointers(),
                                2,
                                writeCount);
                            const auto discard = std::min(
                                capture.latencyToDiscard, writeCount);
                            capture.latencyToDiscard -= discard;
                            const auto processedCount = writeCount - discard;
                            const std::array<const float*, 2> processed {
                                capture.processedBuffer.getReadPointer(0) + discard,
                                capture.processedBuffer.getReadPointer(1) + discard,
                            };
                            recordingCapture->writeAudioTrack(
                                track.id, rawPointer, writeCount,
                                processed.data(), processedCount);
                        }
                        capture.endAudioSample = captureAudioStart
                            + static_cast<std::uint64_t>(writeCount);
                        capture.endTimelineSample = captureTimelineStart
                            + static_cast<std::uint64_t>(writeCount);
                    }
                } else if (capture.state == RecordingCaptureState::capturing) {
                    if (!recordingCapture->beginTailDrain(
                            track.id,
                            capture,
                            track.pluginDelaySamples,
                            track.pluginTailSamples))
                        capture.state = RecordingCaptureState::completed;
                }
            }
            if (track.monitorInput && audible) {
                for (int sample = 0; sample < sampleCount; ++sample) {
                    const auto timelinePosition = rangeStart + sample;
                    const auto gain = juce::Decibels::decibelsToGain(
                        ArrangementGraph::automationValueAt(
                            track.volumeAutomation, timelinePosition, track.gainDb));
                    const auto pan = juce::jlimit(
                        -1.0f,
                        1.0f,
                        ArrangementGraph::automationValueAt(
                            track.panAutomation, timelinePosition, track.pan));
                    const auto panAngle =
                        (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
                    if (channelCount > 0 && outputChannels[0] != nullptr)
                        outputChannels[0][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(0, sample)
                            * gain * std::cos(panAngle);
                    if (channelCount > 1 && outputChannels[1] != nullptr)
                        outputChannels[1][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(1, sample)
                            * gain * std::sin(panAngle);
                }
            }
        }
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        track.mixBuffer.clear();
        track.processedBuffer.clear();
        track.midiBuffer.clear();
        if (track.instrumentRack != nullptr)
            track.instrumentRack->allNotesOff();
        track.effectChain.allNotesOff();
        track.liveEffectChain.allNotesOff();
        track.delayBuffer.clear();
        track.delayWritePosition = 0;
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.recordingCapture);
    }
    recordingCapture->resetDrainingTailTracks();
}

void TimelineEngine::mix(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    mix(nullptr, 0, outputChannels, channelCount, sampleCount);
}

void TimelineEngine::mix(
    const float* const* inputChannels,
    const int inputChannelCount,
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    audioClockSample.fetch_add(static_cast<std::uint64_t>(sampleCount), std::memory_order_relaxed);
    callbackAudioStartSample.store(
        audioClockSample.load(std::memory_order_acquire) - static_cast<std::uint64_t>(sampleCount),
        std::memory_order_release);
    const auto blockPlaybackOffset = juce::jlimit(
        0, sampleCount, playbackBlockOffset.exchange(0, std::memory_order_acq_rel));
    lastMixPlaybackOffset.store(blockPlaybackOffset, std::memory_order_release);
    if (state.load(std::memory_order_acquire) != State::playing) return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked()) {
        callbackLockMisses.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (timeline == nullptr) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    if (recordingCapture->drainingTailTracks() != 0) {
        for (auto& trackPtr : timeline->tracks)
            (void) recordingCapture->drainTail(
                trackPtr->id,
                trackPtr->recordingCapture,
                trackPtr->liveInputBuffer,
                sampleCount);
    }
    if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::stopping) {
        if (recordingCapture->drainingTailTracks() != 0)
            return;
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
        return;
    }
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = sampleCount - consumed;
        if (!timeline->tracks.empty()) {
            const auto bufferSize = timeline->tracks.front()->mixBuffer.getNumSamples();
            if (bufferSize > 0)
                chunk = std::min(chunk, bufferSize);
        }
        if (timeline->loopEnabled && position < timeline->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(timeline->loopEndSample - position));
        for (auto& trackPtr : timeline->tracks)
            trackPtr->mixBuffer.clear(0, chunk);
        for (auto& trackPtr : timeline->tracks)
            mixRange(*trackPtr, position, 0, chunk);
        for (auto& trackPtr : timeline->tracks)
            scheduleMidi(*timeline, *trackPtr, position, chunk);
        const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
        const auto captureSamples = captureBlockSamples.load(std::memory_order_acquire);
        const auto [captureWriteStart, captureWriteEnd] =
            ArrangementGraph::captureIntersection(
                consumed, chunk, captureStart, captureSamples);
        if (captureWriteEnd > captureWriteStart
            && recordingPhase.load(std::memory_order_acquire)
                == RecordingPhase::recording) {
            const auto callbackStart = audioClockSample.load(std::memory_order_acquire)
                - static_cast<std::uint64_t>(sampleCount);
            const auto localOffset = captureWriteStart - consumed;
            recordingCapture->setCaptureRange(
                callbackStart + static_cast<std::uint64_t>(captureWriteStart),
                callbackStart + static_cast<std::uint64_t>(captureWriteEnd),
                static_cast<std::uint64_t>(position)
                    + static_cast<std::uint64_t>(localOffset),
                static_cast<std::uint64_t>(position)
                    + static_cast<std::uint64_t>(
                        localOffset + captureWriteEnd - captureWriteStart));
        }
        processTracks(
            *timeline,
            inputChannels,
            inputChannelCount,
            outputChannels,
            channelCount,
            position,
            consumed,
            chunk);
        position += chunk;
        consumed += chunk;
        // Decrement the capture budget so recording stops at the window end
        {
            auto remaining = captureBlockSamples.load(std::memory_order_acquire);
            if (remaining > 0)
                captureBlockSamples.store(
                    remaining - std::min(chunk, remaining), std::memory_order_release);
        }
        if (timeline->loopEnabled && position >= timeline->loopEndSample) {
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                const auto callbackStart = audioClockSample.load(std::memory_order_acquire)
                    - static_cast<std::uint64_t>(sampleCount);
                recordingCapture->markLoopBoundary(
                    callbackStart + static_cast<std::uint64_t>(consumed));
                for (auto& trackPtr : timeline->tracks) {
                    auto& track = *trackPtr;
                    if (!track.armed || track.instrument
                        || track.recordingCapture.state != RecordingCaptureState::capturing)
                        continue;
                    (void) recordingCapture->endTrackCapture(
                        track.id, track.recordingCapture);
                    track.recordingCapture.state = RecordingCaptureState::idle;
                    track.recordingCapture.effectChain.reset();
                    track.recordingCapture.latencyToDiscard = 0;
                }
            }
            position = timeline->loopStartSample;
            recordingPassOrdinal.fetch_add(1, std::memory_order_relaxed);
            resetPlaybackTrackState(*timeline);
            discontinuity.fetch_add(1, std::memory_order_relaxed);
        }
    }
    timelineSample.store(position, std::memory_order_release);
}

juce::var TimelineEngine::status() const {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "transportStatus");
    const auto currentState = state.load(std::memory_order_acquire);
    object->setProperty(
        "state",
        currentState == State::playing ? "playing" :
        currentState == State::faulted ? "faulted" : "stopped");
    object->setProperty("timelineSample", static_cast<juce::int64>(
        timelineSample.load(std::memory_order_acquire)));
    object->setProperty("audioClockSample", static_cast<juce::int64>(
        audioClockSample.load(std::memory_order_acquire)));
    object->setProperty("sequence", static_cast<juce::int64>(
        sequence.fetch_add(1, std::memory_order_relaxed) + 1));
    object->setProperty("callbackLockMisses", static_cast<juce::int64>(
        callbackLockMisses.load(std::memory_order_acquire)));
    object->setProperty("clockGeneration", static_cast<juce::int64>(
        clockGeneration.load(std::memory_order_acquire)));
    object->setProperty("discontinuity", static_cast<juce::int64>(
        discontinuity.load(std::memory_order_acquire)));
    object->setProperty("revision", 0);
    object->setProperty("sampleRate", 0.0);
    object->setProperty("timelineTick", 0);
    const auto phase = recordingPhase.load(std::memory_order_acquire);
    object->setProperty(
        "recordingPhase",
        phase == RecordingPhase::countingIn ? "countingIn" :
        phase == RecordingPhase::recording ? "recording" :
        phase == RecordingPhase::stopping ? "stopping" : "idle");
    object->setProperty("recordingStartTick", static_cast<juce::int64>(
        recordingStartTick.load(std::memory_order_acquire)));
    object->setProperty("recordingPassOrdinal", static_cast<int>(
        recordingPassOrdinal.load(std::memory_order_acquire)));
    object->setProperty("recordingCaptureErrors", static_cast<juce::int64>(
        recordingCapture->captureErrors()));
    object->setProperty("drainingTailTracks", static_cast<int>(
        recordingCapture->drainingTailTracks()));
    object->setProperty("unavailableClipIds", juce::Array<juce::var> {});
    object->setProperty("missingDeviceIds", juce::Array<juce::var> {});
    juce::Array<juce::var> armedTrackIds;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire),
            timeline->outputSampleRate));
        object->setProperty("timelineTick", tick);
        object->setProperty("recordingCurrentTick", tick);
        object->setProperty("unavailableClipIds", timeline->unavailableClipIds);
        object->setProperty("missingDeviceIds", timeline->missingDeviceIds);
        for (const auto& track : timeline->tracks)
            if (track->armed)
                armedTrackIds.add(track->id);
    }
    object->setProperty("armedTrackIds", armedTrackIds);
    return juce::var(object);
}


} // namespace riffra
