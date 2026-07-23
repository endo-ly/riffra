#include "TimelineEngine.h"
#include "ArrangementGraph.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <fstream>
#include <thread>

namespace riffra {

namespace {
constexpr int kReadAheadSamples = 32768;

bool requiredNumber(const juce::var& object, const juce::Identifier& name, double& value) {
    const auto property = object.getProperty(name, {});
    if (!property.isInt() && !property.isInt64() && !property.isDouble()) return false;
    value = static_cast<double>(property);
    return std::isfinite(value);
}

bool writePcmWave(
    const juce::File& file,
    const std::uint32_t sampleRate,
    const std::uint16_t channels,
    const std::uint32_t frames,
    const std::int16_t sample) {
    std::ofstream stream(file.getFullPathName().toStdString(), std::ios::binary | std::ios::trunc);
    if (!stream) return false;
    const auto dataBytes = frames * channels * static_cast<std::uint32_t>(sizeof(std::int16_t));
    const auto byteRate = sampleRate * channels * static_cast<std::uint32_t>(sizeof(std::int16_t));
    const auto blockAlign = static_cast<std::uint16_t>(channels * sizeof(std::int16_t));
    const auto writeU16 = [&stream](const std::uint16_t value) {
        stream.write(reinterpret_cast<const char*>(&value), sizeof(value));
    };
    const auto writeU32 = [&stream](const std::uint32_t value) {
        stream.write(reinterpret_cast<const char*>(&value), sizeof(value));
    };
    stream.write("RIFF", 4);
    writeU32(36 + dataBytes);
    stream.write("WAVEfmt ", 8);
    writeU32(16);
    writeU16(1);
    writeU16(channels);
    writeU32(sampleRate);
    writeU32(byteRate);
    writeU16(blockAlign);
    writeU16(16);
    stream.write("data", 4);
    writeU32(dataBytes);
    for (std::uint64_t index = 0; index < static_cast<std::uint64_t>(frames) * channels; ++index)
        stream.write(reinterpret_cast<const char*>(&sample), sizeof(sample));
    return stream.good();
}
} // namespace

TimelineEngine::TimelineEngine() { readAheadThread.startThread(); }

TimelineEngine::~TimelineEngine() {
    stop();
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
        pendingTimeline.reset();
    }
    readAheadThread.stopThread(3000);
}

std::int64_t TimelineEngine::tickToSample(
    const std::uint64_t tick,
    const std::uint32_t ppq,
    const double bpm,
    const double sampleRate) noexcept {
    if (ppq == 0 || bpm <= 0.0 || sampleRate <= 0.0) return 0;
    return static_cast<std::int64_t>(std::llround(
        static_cast<double>(tick) * sampleRate * 60.0 /
        (bpm * static_cast<double>(ppq))));
}

bool TimelineEngine::loadSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    juce::String& error,
    const bool commitImmediately) {
    if (!snapshot.isObject() || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }
    auto prepared = std::make_unique<PreparedTimeline>();
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
        !requiredNumber(timebase, "bpm", prepared->bpm) || ppq != 960.0 ||
        prepared->bpm < 20.0 || prepared->bpm > 400.0) {
        error = "Timeline snapshot has an invalid timebase.";
        return false;
    }
    prepared->ppq = static_cast<std::uint32_t>(ppq);
    prepared->outputSampleRate = outputSampleRate;
    const auto denominator = static_cast<int>(timebase.getProperty("timeSignatureDenominator", 4));
    const auto numerator = static_cast<int>(timebase.getProperty("timeSignatureNumerator", 4));
    if (denominator <= 0 || numerator <= 0) {
        error = "Timeline snapshot has an invalid time signature.";
        return false;
    }
    const auto beatTicks = static_cast<double>(prepared->ppq) * 4.0 / denominator;
    prepared->beatSamples = tickToSample(
        static_cast<std::uint64_t>(std::llround(beatTicks)),
        prepared->ppq,
        prepared->bpm,
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
        prepared->loopStartSample = tickToSample(
            startTick, prepared->ppq, prepared->bpm, outputSampleRate);
        prepared->loopEndSample = tickToSample(
            endTick, prepared->ppq, prepared->bpm, outputSampleRate);
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
        prepared->punchStartSample = tickToSample(
            startTick, prepared->ppq, prepared->bpm, outputSampleRate);
        prepared->punchEndSample = tickToSample(
            endTick, prepared->ppq, prepared->bpm, outputSampleRate);
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
    bool monitorLiveInputState = false;
    bool armedInstrumentTrackState = false;
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
        track->gain = juce::Decibels::decibelsToGain(
            static_cast<float>(trackValue.getProperty("gainDb", 0.0)));
        track->pan = juce::jlimit(
            -1.0f, 1.0f, static_cast<float>(trackValue.getProperty("pan", 0.0)));
        track->muted = static_cast<bool>(trackValue.getProperty("muted", false));
        track->solo = static_cast<bool>(trackValue.getProperty("solo", false));
        const auto monitoring = trackValue.getProperty("monitoring", {}).toString();
        track->monitorInput = !track->instrument &&
            (monitoring == "on" || (monitoring == "auto" && track->armed));
        if (track->monitorInput)
            monitorLiveInputState = true;
        const auto audioInput = trackValue.getProperty("audioInput", {});
        if (audioInput.isObject())
            track->audioInputChannel =
                static_cast<int>(audioInput.getProperty("channelIndex", -1));

        const auto rack = trackValue.getProperty("rack", {});
        const auto instrument = trackValue.getProperty("instrument", {});
        const auto devices = rack.isObject()
            ? rack.getProperty("devices", {})
            : juce::var(juce::Array<juce::var> {});
        track->effectConfiguration = juce::JSON::toString(devices, false);
        track->instrumentConfiguration = instrument.isObject()
            ? juce::JSON::toString(instrument, false)
            : juce::String();
        if (commitImmediately) {
            const juce::SpinLock::ScopedLockType lock(timelineLock);
            if (timeline != nullptr) {
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&track](const auto& item) { return item->id == track->id; });
                if (existing != timeline->tracks.end()
                    && (*existing)->effectConfiguration == track->effectConfiguration
                    && (*existing)->instrumentConfiguration == track->instrumentConfiguration) {
                    track->reuseRuntimeDevices = true;
                    track->pluginDelaySamples = (*existing)->pluginDelaySamples;
                }
            }
        }
        if (rack.isObject()) {
            if (!track->reuseRuntimeDevices
                && !track->effectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
            if (!track->reuseRuntimeDevices && !track->instrument &&
                !track->liveEffectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
        }
        if (instrument.isObject() && !track->reuseRuntimeDevices) {
            const auto path = instrument.getProperty("path", {}).toString();
            track->instrumentDeviceId = instrument.getProperty("id", {}).toString();
            track->instrumentRack = std::make_unique<PluginRack>();
            if (const auto loadError =
                    track->instrumentRack->load(path, outputSampleRate, maximumBlockSize)) {
                error = "Track Instrument could not be loaded: " + loadError->message;
                return false;
            }
            const auto stateData = instrument.getProperty("stateData", {}).toString();
            if (stateData.isNotEmpty() &&
                !track->instrumentRack->setState(stateData, error))
                return false;
            if (stateData.isEmpty()) {
                const auto parameters = instrument.getProperty("parameterValues", {});
                const auto status = track->instrumentRack->parameterStatus()
                    .getProperty("parameters", {});
                const auto count = status.isArray() ? status.size() : 0;
                if (parameters.isArray()) {
                    for (int index = 0; index < std::min(parameters.size(), count); ++index)
                        if (!track->instrumentRack->setParameter(
                                index, static_cast<float>(parameters[index]), error))
                            return false;
                }
            }
            track->instrumentRack->setBypassed(
                static_cast<bool>(instrument.getProperty("bypassed", false)));
        }
        if (!track->reuseRuntimeDevices)
            track->pluginDelaySamples = track->effectChain.latencySamples() +
                (track->instrumentRack != nullptr ? track->instrumentRack->latencySamples() : 0);
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
            clip->startSample = tickToSample(
                startTick, prepared->ppq, prepared->bpm, outputSampleRate);
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
            clip->transport.setSource(
                clip->readerSource.get(),
                kReadAheadSamples,
                &readAheadThread,
                clip->sourceSampleRate,
                2);
            clip->transport.prepareToPlay(maximumBlockSize, outputSampleRate);
            clip->transport.start();
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
        prepared->tracks.push_back(std::move(track));
    }
    for (auto& track : prepared->tracks) {
        track->compensationDelaySamples = maximumPluginDelay - track->pluginDelaySamples;
        track->delayBuffer.setSize(
            2, static_cast<int>(track->compensationDelaySamples + maximumBlockSize + 1),
            false, true, false);
        track->delayBuffer.clear();
    }

    if (!commitImmediately) {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        pendingTimeline = std::move(prepared);
        pendingMonitorLiveInput = monitorLiveInputState;
        pendingArmedInstrumentTrack = armedInstrumentTrackState;
        return true;
    }
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        pendingTimeline = std::move(prepared);
        pendingMonitorLiveInput = monitorLiveInputState;
        pendingArmedInstrumentTrack = armedInstrumentTrackState;
    }
    return commitPreparedSnapshot(error);
}

bool TimelineEngine::commitPreparedSnapshot(juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (pendingTimeline == nullptr) {
        error = "No prepared Timeline snapshot is available.";
        return false;
    }
    const auto hasExistingTimeline = timeline != nullptr;
    if (timeline != nullptr) {
        for (auto& candidateTrack : pendingTimeline->tracks) {
            if (!candidateTrack->reuseRuntimeDevices)
                continue;
            const auto existing = std::find_if(
                timeline->tracks.begin(), timeline->tracks.end(),
                [&candidateTrack](const auto& item) {
                    return item->id == candidateTrack->id
                        && item->effectConfiguration == candidateTrack->effectConfiguration
                        && item->instrumentConfiguration
                            == candidateTrack->instrumentConfiguration;
                });
            if (existing == timeline->tracks.end()) {
                error = "Timeline device runtime changed while the snapshot was prepared.";
                return false;
            }
            candidateTrack->effectChain = std::move((*existing)->effectChain);
            candidateTrack->liveEffectChain = std::move((*existing)->liveEffectChain);
            candidateTrack->instrumentRack = std::move((*existing)->instrumentRack);
        }
    }
    timeline = std::move(pendingTimeline);
    monitorLiveInput.store(pendingMonitorLiveInput, std::memory_order_release);
    armedInstrumentTrack.store(pendingArmedInstrumentTrack, std::memory_order_release);
    if (!hasExistingTimeline)
        timelineSample.store(0, std::memory_order_release);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
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
    if (lock.isLocked() && timeline != nullptr)
        resetTrackState(*timeline);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr)
        resetTrackState(*timeline);
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    timelineSample.store(
        tickToSample(tick, timeline->ppq, timeline->bpm, timeline->outputSampleRate),
        std::memory_order_release);
    resetTrackState(*timeline);
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
    recordingPassOrdinal.store(1, std::memory_order_release);
    if (state.load(std::memory_order_acquire) == State::playing || countInBeats <= 0) {
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        recordingStartAudioSample.store(
            audioClockSample.load(std::memory_order_acquire), std::memory_order_release);
        const auto tick = timeline->outputSampleRate > 0.0
            ? static_cast<std::uint64_t>(std::llround(
                static_cast<double>(timelineSample.load(std::memory_order_acquire)) *
                timeline->bpm * static_cast<double>(timeline->ppq) /
                (timeline->outputSampleRate * 60.0)))
            : 0;
        recordingStartTick.store(tick, std::memory_order_release);
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
    sequence.fetch_add(1, std::memory_order_relaxed);
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
}

juce::var TimelineEngine::recordingConfiguration() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr)
        return {};
    auto* result = new juce::DynamicObject();
    result->setProperty("sampleRate", timeline->outputSampleRate);
    const auto tick = timeline->outputSampleRate > 0.0
        ? static_cast<juce::int64>(std::llround(
            static_cast<double>(timelineSample.load(std::memory_order_acquire))
            * timeline->bpm * static_cast<double>(timeline->ppq)
            / (timeline->outputSampleRate * 60.0)))
        : 0;
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
        trackValues.add(juce::var(value));
    }
    result->setProperty("tracks", trackValues);
    return juce::var(result);
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingSink.store(sink, std::memory_order_release);
}

void TimelineEngine::clearRecordingSink() noexcept {
    recordingSink.store(nullptr, std::memory_order_release);
    while (recordingSinkReaders.load(std::memory_order_acquire) != 0)
        std::this_thread::yield();
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
                recordingSinkReaders.fetch_add(1, std::memory_order_acq_rel);
                if (auto* sink = recordingSink.load(std::memory_order_acquire))
                    sink->writeMidiTrack(
                        track.id, deviceId, message,
                        audioClockSample.load(std::memory_order_acquire));
                recordingSinkReaders.fetch_sub(1, std::memory_order_acq_rel);
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
        if (playback == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        playback->setBypassed(bypassed);
        if (live != nullptr)
            live->setBypassed(bypassed);
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
    if (sampleCount <= 0)
        return false;
    auto phase = recordingPhase.load(std::memory_order_acquire);
    if (phase == RecordingPhase::idle || phase == RecordingPhase::stopping) {
        capturedSamples = 0;
        return false;
    }
    if (phase == RecordingPhase::countingIn) {
        const auto remaining = countInRemainingSamples.load(std::memory_order_acquire);
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
            const auto tick = timeline->outputSampleRate > 0.0
                ? static_cast<std::uint64_t>(std::llround(
                    static_cast<double>(timelineSample.load(std::memory_order_acquire)) *
                    timeline->bpm * static_cast<double>(timeline->ppq) /
                    (timeline->outputSampleRate * 60.0)))
                : 0;
            recordingStartTick.store(tick, std::memory_order_release);
        }
        state.store(State::playing, std::memory_order_release);
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        phase = RecordingPhase::recording;
    }

    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->punchEnabled) {
        captureBlockOffset.store(sampleOffset, std::memory_order_release);
        captureBlockSamples.store(capturedSamples, std::memory_order_release);
        if (auto* sink = recordingSink.load(std::memory_order_acquire))
            sink->setCaptureRange(
                audioClockSample.load(std::memory_order_acquire)
                    + static_cast<std::uint64_t>(sampleOffset),
                audioClockSample.load(std::memory_order_acquire)
                    + static_cast<std::uint64_t>(sampleOffset + capturedSamples));
        return true;
    }

    const auto position = timelineSample.load(std::memory_order_acquire);
    const auto blockEnd = position + static_cast<std::int64_t>(sampleCount);
    if (blockEnd <= timeline->punchStartSample || position >= timeline->punchEndSample) {
        capturedSamples = 0;
        return false;
    }
    sampleOffset = static_cast<int>(std::max<std::int64_t>(
        0, timeline->punchStartSample - position));
    const auto end = std::min<std::int64_t>(blockEnd, timeline->punchEndSample);
    capturedSamples = static_cast<int>(std::max<std::int64_t>(
        0, end - position - sampleOffset));
    captureBlockOffset.store(sampleOffset, std::memory_order_release);
    captureBlockSamples.store(capturedSamples, std::memory_order_release);
    if (capturedSamples > 0)
        if (auto* sink = recordingSink.load(std::memory_order_acquire))
            sink->setCaptureRange(
                audioClockSample.load(std::memory_order_acquire)
                    + static_cast<std::uint64_t>(sampleOffset),
                audioClockSample.load(std::memory_order_acquire)
                    + static_cast<std::uint64_t>(sampleOffset + capturedSamples));
    return capturedSamples > 0;
}

void TimelineEngine::mixMetronome(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    if (state.load(std::memory_order_acquire) != State::playing || sampleCount <= 0)
        return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->metronomeEnabled
        || timeline->beatSamples <= 0)
        return;
    const auto loopLength = timeline->loopEndSample - timeline->loopStartSample;
    const auto start = lastMixStartSample.load(std::memory_order_acquire);
    constexpr std::int64_t clickSamples = 1'920;
    for (int sample = 0; sample < sampleCount; ++sample) {
        auto position = start + sample;
        if (timeline->loopEnabled && loopLength > 0 && position >= timeline->loopEndSample)
            position = timeline->loopStartSample +
                (position - timeline->loopEndSample) % loopLength;
        if (position < 0)
            continue;
        const auto beat = position / timeline->beatSamples;
        const auto offset = position % timeline->beatSamples;
        if (offset < 0 || offset >= clickSamples)
            continue;
        const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
        const auto amplitude = beat % timeline->beatsPerBar == 0 ? 0.18f : 0.11f;
        const auto value = amplitude * envelope;
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
                clip.transport.setPosition(
                    static_cast<double>(sourceFrame) / clip.sourceSampleRate);
            }
            clip.scratch.clear();
            clip.transport.getNextAudioBlock(
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
    Track& track,
    const std::int64_t rangeStart,
    const int sampleCount) noexcept {
    track.midiBuffer.clear();
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : track.midiClips) {
        if (clip.muted) continue;
        const auto clipStart = tickToSample(
            clip.startTick, timeline != nullptr ? timeline->ppq : 960,
            timeline != nullptr ? timeline->bpm : 120.0, track.outputSampleRate);
        const auto clipLength = std::max<std::int64_t>(1, tickToSample(
            clip.durationTicks, timeline != nullptr ? timeline->ppq : 960,
            timeline != nullptr ? timeline->bpm : 120.0, track.outputSampleRate));
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
                const auto noteStart = iterationStart + tickToSample(
                    note.startTick, timeline != nullptr ? timeline->ppq : 960,
                    timeline != nullptr ? timeline->bpm : 120.0, track.outputSampleRate);
                const auto noteEnd = std::min(
                    iterationStart + clipLength,
                    noteStart + std::max<std::int64_t>(1, tickToSample(
                        note.durationTicks, timeline != nullptr ? timeline->ppq : 960,
                        timeline != nullptr ? timeline->bpm : 120.0, track.outputSampleRate)));
                addMessage(juce::MidiMessage::noteOn(
                    juce::jlimit(1, 16, note.channel), note.note,
                    static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))), noteStart);
                addMessage(juce::MidiMessage::noteOff(
                    juce::jlimit(1, 16, note.channel), note.note), noteEnd);
            }
            for (const auto& event : clip.events) {
                const auto eventSample = iterationStart + tickToSample(
                    event.tick, timeline != nullptr ? timeline->ppq : 960,
                    timeline != nullptr ? timeline->bpm : 120.0, track.outputSampleRate);
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
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(
        prepared.tracks.begin(), prepared.tracks.end(),
        [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.muted || (hasSolo && !track.solo)) continue;
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
        const auto panAngle = (track.pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const auto leftGain = track.gain * std::cos(panAngle);
        const auto rightGain = track.gain * std::sin(panAngle);
        const auto delay = track.compensationDelaySamples;
        const auto delaySize = track.delayBuffer.getNumSamples();
        for (int sample = 0; sample < sampleCount; ++sample) {
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
            if (channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + sample] += left * leftGain;
            if (channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + sample] += right * rightGain;
        }
        if (!track.instrument && (track.monitorInput || track.armed)
            && track.audioInputChannel >= 0) {
            const auto* source = track.audioInputChannel < physicalInputChannelCount
                ? physicalInputChannels[track.audioInputChannel]
                : nullptr;
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
            if (track.armed && writeEnd > writeStart) {
                recordingSinkReaders.fetch_add(1, std::memory_order_acq_rel);
                if (auto* sink = recordingSink.load(std::memory_order_acquire)) {
                    const auto localOffset = writeStart - destinationStart;
                    const std::array<const float*, 2> processed {
                        track.liveProcessedBuffer.getReadPointer(0) + localOffset,
                        track.liveProcessedBuffer.getReadPointer(1) + localOffset,
                    };
                    sink->writeAudioTrack(
                        track.id,
                        track.liveInputBuffer.getReadPointer(0) + localOffset,
                        processed.data(),
                        writeEnd - writeStart);
                }
                recordingSinkReaders.fetch_sub(1, std::memory_order_acq_rel);
            }
            if (track.monitorInput) {
                for (int sample = 0; sample < sampleCount; ++sample) {
                    if (channelCount > 0 && outputChannels[0] != nullptr)
                        outputChannels[0][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(0, sample) * leftGain;
                    if (channelCount > 1 && outputChannels[1] != nullptr)
                        outputChannels[1][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(1, sample) * rightGain;
                }
            }
        }
    }
}

void TimelineEngine::resetTrackState(PreparedTimeline& prepared) noexcept {
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
    if (state.load(std::memory_order_acquire) != State::playing) return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    auto consumed = juce::jlimit(
        0, sampleCount, playbackBlockOffset.exchange(0, std::memory_order_acq_rel));
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
            scheduleMidi(*trackPtr, position, chunk);
        processTracks(
            *timeline,
            inputChannels,
            inputChannelCount,
            outputChannels,
            channelCount,
            consumed,
            chunk);
        position += chunk;
        consumed += chunk;
        if (timeline->loopEnabled && position >= timeline->loopEndSample) {
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                recordingSinkReaders.fetch_add(1, std::memory_order_acq_rel);
                if (auto* sink = recordingSink.load(std::memory_order_acquire)) {
                    const auto callbackStart = audioClockSample.load(std::memory_order_acquire)
                        - static_cast<std::uint64_t>(sampleCount);
                    sink->markLoopBoundary(
                        callbackStart + static_cast<std::uint64_t>(consumed));
                }
                recordingSinkReaders.fetch_sub(1, std::memory_order_acq_rel);
            }
            position = timeline->loopStartSample;
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording)
                recordingPassOrdinal.fetch_add(1, std::memory_order_relaxed);
            resetTrackState(*timeline);
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
    object->setProperty("unavailableClipIds", juce::Array<juce::var> {});
    object->setProperty("missingDeviceIds", juce::Array<juce::var> {});
    juce::Array<juce::var> armedTrackIds;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        const auto tick = timeline->outputSampleRate > 0.0
            ? static_cast<juce::int64>(std::llround(
                static_cast<double>(timelineSample.load(std::memory_order_acquire)) *
                timeline->bpm * static_cast<double>(timeline->ppq) /
                (timeline->outputSampleRate * 60.0)))
            : 0;
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

juce::var runTimelineSelfTest(const juce::File& directory) {
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "timelineSelfTest");
    juce::Array<juce::var> checks;
    const auto mono = directory.getChildFile("timeline-44100-mono.wav");
    const auto stereo = directory.getChildFile("timeline-48000-stereo.wav");
    directory.createDirectory();
    const auto sourcesWritten =
        writePcmWave(mono, 44100, 1, 44100, 6000) &&
        writePcmWave(stereo, 48000, 2, 48000, 4000);

    bool loaded = false;
    bool mixed = false;
    bool seeked = false;
    bool looped = false;
    bool punchWindowed = false;
    bool metronomeMixed = false;
    juce::String error;
    if (sourcesWritten) {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        auto* timebase = new juce::DynamicObject();
        timebase->setProperty("ppq", 960);
        timebase->setProperty("bpm", 120.0);
        timebase->setProperty("timeSignatureNumerator", 4);
        timebase->setProperty("timeSignatureDenominator", 4);
        auto* loopRange = new juce::DynamicObject();
        loopRange->setProperty("enabled", false);
        loopRange->setProperty("startTick", 0);
        loopRange->setProperty("endTick", 0);
        juce::Array<juce::var> clips;
        const auto addClip = [&clips](
            const juce::String& id,
            const juce::File& file,
            const int sampleRate,
            const int frames) {
            auto* clip = new juce::DynamicObject();
            clip->setProperty("clipId", id);
            clip->setProperty("path", file.getFullPathName());
            clip->setProperty("sourceSampleRate", sampleRate);
            clip->setProperty("sourceStartFrame", 0);
            clip->setProperty("sourceEndFrame", frames);
            clip->setProperty("durationFrames", frames);
            clip->setProperty("durationSampleRate", sampleRate);
            clip->setProperty("startTick", 0);
            clip->setProperty("fadeInFrames", 0);
            clip->setProperty("fadeOutFrames", 0);
            clip->setProperty("gainDb", 0.0);
            clip->setProperty("pan", 0.0);
            clip->setProperty("loopEnabled", false);
            clip->setProperty("muted", false);
            clips.add(juce::var(clip));
        };
        addClip("mono-44100", mono, 44100, 44100);
        addClip("stereo-48000", stereo, 48000, 48000);
        auto* audioTrack = new juce::DynamicObject();
        audioTrack->setProperty("id", "track:test");
        audioTrack->setProperty("gainDb", 0.0);
        audioTrack->setProperty("pan", 0.0);
        audioTrack->setProperty("muted", false);
        audioTrack->setProperty("solo", false);
        auto* rack = new juce::DynamicObject();
        rack->setProperty("devices", juce::Array<juce::var> {});
        audioTrack->setProperty("rack", juce::var(rack));
        audioTrack->setProperty("audioClips", clips);
        audioTrack->setProperty("midiClips", juce::Array<juce::var> {});
        juce::Array<juce::var> tracks;
        tracks.add(juce::var(audioTrack));
        auto* snapshotObject = new juce::DynamicObject();
        snapshotObject->setProperty("revision", 7);
        snapshotObject->setProperty("timebase", juce::var(timebase));
        snapshotObject->setProperty("loopRange", juce::var(loopRange));
        snapshotObject->setProperty("tracks", tracks);
        loaded = engine.loadSnapshot(
            juce::var(snapshotObject), formats, 48000.0, 512, error);
        if (loaded) {
            engine.play();
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            std::array<float, 512> left {};
            std::array<float, 512> right {};
            std::array<float*, 2> channels { left.data(), right.data() };
            for (int block = 0; block < 8; ++block)
                engine.mix(channels.data(), 2, static_cast<int>(left.size()));
            const auto peak = std::max(
                *std::max_element(left.begin(), left.end()),
                *std::max_element(right.begin(), right.end()));
            mixed = peak > 0.1f;
            engine.seekToTick(960);
            const auto seekStatus = engine.status();
            seeked = static_cast<juce::int64>(seekStatus.getProperty("timelineSample", -1)) == 24000;

            auto* loopSnapshot = new juce::DynamicObject();
            auto* loopTimebase = new juce::DynamicObject();
            loopTimebase->setProperty("ppq", 960);
            loopTimebase->setProperty("bpm", 120.0);
            loopTimebase->setProperty("timeSignatureNumerator", 4);
            loopTimebase->setProperty("timeSignatureDenominator", 4);
            auto* enabledLoop = new juce::DynamicObject();
            enabledLoop->setProperty("enabled", true);
            enabledLoop->setProperty("startTick", 0);
            enabledLoop->setProperty("endTick", 960);
            auto* punchRange = new juce::DynamicObject();
            punchRange->setProperty("startTick", 480);
            punchRange->setProperty("endTick", 960);
            loopSnapshot->setProperty("revision", 8);
            loopSnapshot->setProperty("timebase", juce::var(loopTimebase));
            loopSnapshot->setProperty("loopRange", juce::var(enabledLoop));
            loopSnapshot->setProperty("punchRange", juce::var(punchRange));
            loopSnapshot->setProperty("metronomeEnabled", true);
            auto* loopTrack = new juce::DynamicObject();
            loopTrack->setProperty("id", "track:loop");
            loopTrack->setProperty("gainDb", 0.0);
            loopTrack->setProperty("pan", 0.0);
            loopTrack->setProperty("muted", false);
            loopTrack->setProperty("solo", false);
            auto* loopRack = new juce::DynamicObject();
            loopRack->setProperty("devices", juce::Array<juce::var> {});
            loopTrack->setProperty("rack", juce::var(loopRack));
            loopTrack->setProperty("audioClips", juce::Array<juce::var> {});
            loopTrack->setProperty("midiClips", juce::Array<juce::var> {});
            juce::Array<juce::var> loopTracks;
            loopTracks.add(juce::var(loopTrack));
            loopSnapshot->setProperty("tracks", loopTracks);
            if (engine.loadSnapshot(
                    juce::var(loopSnapshot), formats, 48000.0, 512, error)) {
                int punchOffset = 0;
                int punchSamples = 0;
                engine.seekToTick(480);
                engine.startRecording(0, error);
                punchWindowed = engine.recordingWindow(512, punchOffset, punchSamples)
                    && punchOffset == 0 && punchSamples == 512;
                engine.stopRecording();
                engine.seekToTick(0);
                std::array<float, 24000> silent {};
                std::array<float*, 1> silentChannels { silent.data() };
                engine.mix(silentChannels.data(), 1, static_cast<int>(silent.size()));
                looped = static_cast<juce::int64>(
                    engine.status().getProperty("timelineSample", -1)) == 0;
                std::array<float, 512> clicks {};
                std::array<float*, 1> clickChannels { clicks.data() };
                engine.mixMetronome(clickChannels.data(), 1, static_cast<int>(clicks.size()));
                metronomeMixed = *std::max_element(clicks.begin(), clicks.end()) > 0.0f;
            }
        }
    }

    const auto addCheck = [&checks](const juce::String& name, const bool passed) {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", name);
        check->setProperty("passed", passed);
        checks.add(juce::var(check));
    };
    addCheck("44.1 kHz mono and 48 kHz stereo sources load", sourcesWritten && loaded);
    addCheck("overlapping sources mix through read-ahead and sample-rate correction", mixed);
    addCheck("tick seek resolves against the engine sample clock", seeked);
    addCheck("loop wrap returns to the exact loop start", looped);
    addCheck("punch range limits the recording window", punchWindowed);
    addCheck("metronome follows the timeline clock", metronomeMixed);
    result->setProperty("checks", checks);
    result->setProperty("message", error);
    result->setProperty(
        "passed", sourcesWritten && loaded && mixed && seeked && looped && punchWindowed
            && metronomeMixed);
    mono.deleteFile();
    stereo.deleteFile();
    return juce::var(result);
}

} // namespace riffra
