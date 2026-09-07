#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <limits>
#include <memory>
#include <thread>
#include <vector>

#include "ArrangeRecordingSession.h"
#include "OfflineRenderer.h"
#include "SafetyAudioCallback.h"
#include "TestAudioProcessor.h"
#include "TestSupport.h"
#include "TimelineEngine.h"
#include "TimelineTestSupport.h"
#include "instrument/Vst3InstrumentRuntime.h"

namespace riffra {

namespace {

juce::var makeInstrumentSnapshot(const juce::String& trackId,
                                 const juce::String& instrumentDeviceId = {}) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* track = new juce::DynamicObject();
    track->setProperty("id", trackId);
    track->setProperty("kind", "instrument");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", false);
    track->setProperty("monitoring", "off");
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", juce::Array<juce::var>{});
    track->setProperty("midiClips", juce::Array<juce::var>{});
    track->setProperty("automation", juce::Array<juce::var>{});
    if (instrumentDeviceId.isNotEmpty()) {
        auto* instrument = new juce::DynamicObject();
        instrument->setProperty("id", instrumentDeviceId);
        instrument->setProperty("type", "vst3");
        instrument->setProperty("disabledPlaceholder", true);
        track->setProperty("instrument", juce::var(instrument));
    }

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

juce::var makeBuiltInInstrumentSnapshot(const juce::String& trackId, const bool loopEnabled = false,
                                        const bool armed = false) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    const auto preset =
        juce::File(RIFFRA_SONALLOY_TEST_PRESET_ROOT).getChildFile("01-clean-sub-bass");
    auto* instrument = new juce::DynamicObject();
    instrument->setProperty("id", "instrument:clean-sub-bass");
    instrument->setProperty("type", "internal");
    instrument->setProperty("resourceType", "builtInPreset");
    instrument->setProperty("presetId", "01-clean-sub-bass");
    instrument->setProperty("definitionJson",
                            preset.getChildFile("definition.json").loadFileAsString());
    instrument->setProperty("definitionBaseDir", preset.getFullPathName());
    instrument->setProperty("bypassed", false);

    auto* note = new juce::DynamicObject();
    note->setProperty("startTick", 0);
    note->setProperty("durationTicks", 120);
    note->setProperty("note", 60);
    note->setProperty("velocity", 100);
    note->setProperty("channel", 1);
    juce::Array<juce::var> notes;
    notes.add(juce::var(note));

    auto* midiClip = new juce::DynamicObject();
    midiClip->setProperty("startTick", 0);
    midiClip->setProperty("durationTicks", loopEnabled ? 240 : 960);
    midiClip->setProperty("loopEnabled", loopEnabled);
    midiClip->setProperty("muted", false);
    midiClip->setProperty("notes", notes);
    midiClip->setProperty("events", juce::Array<juce::var>{});
    juce::Array<juce::var> midiClips;
    midiClips.add(juce::var(midiClip));

    auto* track = new juce::DynamicObject();
    track->setProperty("id", trackId);
    track->setProperty("kind", "instrument");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", armed);
    track->setProperty("monitoring", "off");
    track->setProperty("instrument", juce::var(instrument));
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", juce::Array<juce::var>{});
    track->setProperty("midiClips", midiClips);
    track->setProperty("automation", juce::Array<juce::var>{});

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    if (loopEnabled) {
        auto* loopRange = new juce::DynamicObject();
        loopRange->setProperty("enabled", true);
        loopRange->setProperty("startTick", 0);
        loopRange->setProperty("endTick", 240);
        snapshot->setProperty("loopRange", juce::var(loopRange));
    }
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

juce::var makeAudioTrackSnapshot(const int trackCount, const bool monitorFirstTrack,
                                 const bool armFirstTrack) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    juce::Array<juce::var> tracks;
    for (int index = 0; index < trackCount; ++index) {
        const auto primary = index == 0;
        auto* track = new juce::DynamicObject();
        track->setProperty("id", primary ? juce::String("track:live")
                                         : juce::String("track:unrelated-") + juce::String(index));
        track->setProperty("kind", "audio");
        track->setProperty("gainDb", 0.0);
        track->setProperty("pan", 0.0);
        track->setProperty("muted", false);
        track->setProperty("solo", false);
        track->setProperty("armed", primary && armFirstTrack);
        track->setProperty("monitoring", primary && monitorFirstTrack ? "on" : "off");
        auto* audioInput = new juce::DynamicObject();
        audioInput->setProperty("channelIndex", 0);
        track->setProperty("audioInput", juce::var(audioInput));
        auto* rack = new juce::DynamicObject();
        rack->setProperty("devices", juce::Array<juce::var>{});
        track->setProperty("rack", juce::var(rack));
        track->setProperty("audioClips", juce::Array<juce::var>{});
        track->setProperty("midiClips", juce::Array<juce::var>{});
        track->setProperty("automation", juce::Array<juce::var>{});
        tracks.add(juce::var(track));
    }

    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

juce::var makeRawAndProcessedClipSnapshot(const juce::File& rawFile,
                                          const juce::File& processedFile) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    juce::Array<juce::var> clips;
    const auto addClip = [&clips](const juce::String& id, const juce::File& file,
                                  const juce::String& takeVariant) {
        auto* clip = new juce::DynamicObject();
        clip->setProperty("clipId", id);
        clip->setProperty("path", file.getFullPathName());
        clip->setProperty("sourceSampleRate", 48'000);
        clip->setProperty("sourceStartFrame", 0);
        clip->setProperty("sourceEndFrame", 32);
        clip->setProperty("durationFrames", 32);
        clip->setProperty("durationSampleRate", 48'000);
        clip->setProperty("startTick", 0);
        clip->setProperty("fadeInFrames", 0);
        clip->setProperty("fadeOutFrames", 0);
        clip->setProperty("fadeShape", 1);
        clip->setProperty("gainDb", 0.0);
        clip->setProperty("pan", 0.0);
        clip->setProperty("takeVariant", takeVariant);
        clip->setProperty("loopEnabled", false);
        clip->setProperty("muted", false);
        clips.add(juce::var(clip));
    };
    addClip("clip:raw", rawFile, "raw");
    addClip("clip:processed", processedFile, "processed");

    auto* track = new juce::DynamicObject();
    track->setProperty("id", "track:audio");
    track->setProperty("kind", "audio");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", false);
    track->setProperty("monitoring", "off");
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", clips);
    track->setProperty("midiClips", juce::Array<juce::var>{});
    track->setProperty("automation", juce::Array<juce::var>{});

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

}  // namespace

class TimelineEngineTestPeer final {
public:
    static bool addChainDevice(PluginChain& chain, const juce::String& id,
                               std::unique_ptr<juce::AudioProcessor> processor,
                               const double sampleRate, const int blockSize, juce::String& error) {
        auto rack = PluginRackTestPeer::install(std::move(processor), sampleRate, blockSize, error);
        if (rack == nullptr) return false;
        chain.devices.push_back(PluginChain::Device{id, std::move(rack)});
        chain.prepare(sampleRate, blockSize);
        return true;
    }

    static bool installTrackChainDevice(TimelineEngine& engine, const juce::String& trackId,
                                        const juce::String& deviceId,
                                        std::unique_ptr<juce::AudioProcessor> processor,
                                        const double sampleRate, const int blockSize,
                                        juce::String& error) {
        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
        if (engine.timeline == nullptr) return false;
        const auto found =
            std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == engine.timeline->tracks.end()) return false;
        auto& track = *(*found);
        return track.runtime != nullptr &&
               addChainDevice(track.runtime->effects(), deviceId, std::move(processor), sampleRate,
                              blockSize, error);
    }

    static bool installTrackInstrument(TimelineEngine& engine, const juce::String& trackId,
                                       std::unique_ptr<PluginRack> rack) {
        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
        if (engine.timeline == nullptr) return false;
        const auto found =
            std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == engine.timeline->tracks.end() || (*found)->runtime == nullptr) return false;
        (*found)->runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        return true;
    }

    static bool setPlaybackCompensationForTest(TimelineEngine& engine, const juce::String& trackId,
                                               const std::int64_t samples) {
        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
        if (engine.timeline == nullptr) return false;
        const auto found =
            std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == engine.timeline->tracks.end()) return false;
        auto& track = *(*found);
        if (track.runtime == nullptr) return false;
        track.runtime->compensationDelaySamples = samples;
        track.runtime->postEffectCompensationDelaySamples = samples;
        const auto bufferSize = static_cast<int>(samples + track.runtime->preparedBlockSize + 1);
        track.runtime->delayBuffer.setSize(2, bufferSize, false, true, false);
        track.runtime->delayBuffer.clear();
        track.runtime->postEffectDelayBuffer.setSize(2, bufferSize, false, true, false);
        track.runtime->postEffectDelayBuffer.clear();
        return true;
    }

    static bool cachePluginTailForTest(TimelineEngine& engine, const juce::String& trackId) {
        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
        if (engine.timeline == nullptr) return false;
        const auto found =
            std::find_if(engine.timeline->tracks.begin(), engine.timeline->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == engine.timeline->tracks.end() || (*found)->runtime == nullptr) return false;
        auto& runtime = *(*found)->runtime;
        runtime.pluginTailSamples = runtime.totalPluginTailSamples();
        return true;
    }

    static bool trackEffectChainProcessesOnce() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(makeInstrumentSnapshot("track:effect-chain"), formats, 48'000.0,
                                 32, error))
            return false;
        std::vector<int> processOrder;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& track = *engine.timeline->tracks.front();
            if (track.runtime == nullptr ||
                !addChainDevice(track.runtime->effects(), "effect:first",
                                std::make_unique<TestChainProcessor>(1, 1.0f, 0, processOrder),
                                48'000.0, 32, error) ||
                !addChainDevice(track.runtime->effects(), "effect:second",
                                std::make_unique<TestChainProcessor>(2, 1.0f, 0, processOrder),
                                48'000.0, 32, error))
                return false;
        }

        // Act
        engine.play();
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return processOrder == std::vector<int>{1, 2};
    }

    static bool canonicalTrackStateSurvivesReusableDeviceCommit() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        const auto first = makeInstrumentSnapshot("track:state", "instrument:state");
        if (!engine.loadSnapshot(first, formats, 48'000.0, 32, error)) return false;
        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                                48'000.0, 32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        if (!TimelineEngineTestPeer::installTrackInstrument(engine, "track:state", std::move(rack)))
            return false;

        auto second = makeInstrumentSnapshot("track:state", "instrument:state");
        auto* secondObject = second.getDynamicObject();
        if (secondObject == nullptr) return false;
        auto tracks = secondObject->getProperty("tracks");
        if (!tracks.isArray() || tracks.size() != 1) return false;
        auto* track = tracks[0].getDynamicObject();
        if (track == nullptr) return false;
        track->setProperty("gainDb", -6.0);
        track->setProperty("pan", 0.5);
        track->setProperty("muted", true);
        track->setProperty("solo", true);
        track->setProperty("armed", true);

        auto* note = new juce::DynamicObject();
        note->setProperty("startTick", 0);
        note->setProperty("durationTicks", 120);
        note->setProperty("note", 64);
        note->setProperty("velocity", 100);
        note->setProperty("channel", 1);
        juce::Array<juce::var> notes;
        notes.add(juce::var(note));
        auto* midiClip = new juce::DynamicObject();
        midiClip->setProperty("startTick", 0);
        midiClip->setProperty("durationTicks", 960);
        midiClip->setProperty("loopEnabled", false);
        midiClip->setProperty("muted", false);
        midiClip->setProperty("notes", notes);
        midiClip->setProperty("events", juce::Array<juce::var>{});
        juce::Array<juce::var> midiClips;
        midiClips.add(juce::var(midiClip));
        track->setProperty("midiClips", midiClips);

        auto* point = new juce::DynamicObject();
        point->setProperty("tick", 480);
        point->setProperty("value", -3.0);
        juce::Array<juce::var> points;
        points.add(juce::var(point));
        auto* lane = new juce::DynamicObject();
        lane->setProperty("parameter", "volume");
        lane->setProperty("points", points);
        juce::Array<juce::var> automation;
        automation.add(juce::var(lane));
        track->setProperty("automation", automation);
        secondObject->setProperty("revision", 2);

        // Act
        if (!engine.loadSnapshot(second, formats, 48'000.0, 32, error, false) ||
            !engine.preparedTrackReusesRuntimeDevices("track:state") ||
            !engine.commitPreparedSnapshot(error))
            return false;

        // Assert
        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
        if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
        const auto& runtime = *engine.timeline->tracks.front()->runtime;
        return runtime.instrument() != nullptr && runtime.instrument()->vst3Rack() == rackPointer &&
               runtime.gainDb == -6.0f && runtime.pan == 0.5f && runtime.muted && runtime.solo &&
               runtime.armed && runtime.midiClips.size() == 1 && !runtime.volumeAutomation.empty();
    }

    static bool editorParameterUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(
                makeInstrumentSnapshot("track:editor-instrument", "instrument:editor"), formats,
                48'000.0, 32, error))
            return false;
        auto rack = PluginRackTestPeer::install(std::make_unique<StateTestProcessor>(), 48'000.0,
                                                32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& track = *engine.timeline->tracks.front();
            if (track.runtime == nullptr) return false;
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        if (!engine.mirrorEditorDeviceParameter("track:editor-instrument", "instrument:editor", 0,
                                                0.75f, error))
            return false;
        engine.play();
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        const auto state = rackPointer->persistedState(error);
        const auto values = state.getProperty("parameterValues", {});
        return values.isArray() && values.size() > 0 &&
               std::abs(static_cast<float>(values[0]) - 0.75f) <= 0.0001f;
    }

    static bool persistedStateUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(
                makeInstrumentSnapshot("track:plugin-state", "instrument:plugin-state"), formats,
                48'000.0, 32, error))
            return false;

        auto rack = PluginRackTestPeer::install(std::make_unique<StateTestProcessor>(), 48'000.0,
                                                32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& track = *engine.timeline->tracks.front();
            if (track.runtime == nullptr) return false;
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }
        if (!rackPointer->setParameter(0, 0.25f, error)) return false;

        auto* desiredState = new juce::DynamicObject();
        desiredState->setProperty("parameterValues", juce::Array<juce::var>{0.75f});
        desiredState->setProperty("bypassed", false);
        // Act
        const auto changed = engine.setDevicePersistedState(
            "track:plugin-state", "instrument:plugin-state", juce::var(desiredState), error);

        // Assert
        if (!changed) return false;
        const auto restored = rackPointer->persistedState(error);
        const auto values = restored.getProperty("parameterValues", {});
        return values.isArray() && values.size() > 0 &&
               std::abs(static_cast<float>(values[0]) - 0.75f) <= 0.0001f;
    }

    static bool programChangeUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(
                makeInstrumentSnapshot("track:plugin-program", "instrument:plugin-program"),
                formats, 48'000.0, 32, error))
            return false;

        ProcessorTrace trace;
        auto rack = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), 48'000.0,
                                                32, error);
        if (rack == nullptr) return false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& track = *engine.timeline->tracks.front();
            if (track.runtime == nullptr) return false;
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        const auto changed =
            engine.setDeviceProgram("track:plugin-program", "instrument:plugin-program", 1, error);

        // Assert
        return changed && trace.currentProgram == 1;
    }

    static bool liveInstrumentProcessesWhileStopped() {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(makeInstrumentSnapshot("track:live-instrument"), formats, 48'000.0,
                                 32, error))
            return false;

        InstrumentTrace trace;
        auto instrumentRack = PluginRackTestPeer::install(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (instrumentRack == nullptr) return false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& liveTrack = *engine.timeline->tracks.front();
            if (liveTrack.runtime == nullptr) return false;
            liveTrack.runtime->setInstrument(
                Vst3InstrumentRuntime::fromRack(std::move(instrumentRack)));
            // Simulate a Project where another Track's plugin is the latency
            // leader. Live MIDI remains immediate even when Timeline MIDI is
            // compensated on the same Track.
            liveTrack.runtime->pluginDelaySamples = 0;
            liveTrack.runtime->compensationDelaySamples = 4;
        }

        if (!engine.enqueueTargetedMidi("track:live-instrument",
                                        juce::MidiMessage::noteOn(1, 60, 0.8f), error))
            return false;

        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
        const auto peak = std::max(*std::max_element(left.begin(), left.end()),
                                   *std::max_element(right.begin(), right.end()));
        // The live voice must start at the first output sample instead of being
        // pushed right by the inter-track compensation delay.
        const auto immediate = std::max(left[0], right[0]);
        return trace.lastMidiMessage.isNoteOn() && trace.noteHeld && peak > 0.0f &&
               immediate > 0.0f;
    }

    static bool timelineMidiKeepsPdcWhenLiveMidiIsImmediate() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        auto snapshot = makeInstrumentSnapshot("track:pdc", "instrument:pdc");
        auto* snapshotObject = snapshot.getDynamicObject();
        if (snapshotObject == nullptr) return false;
        auto tracks = snapshotObject->getProperty("tracks");
        if (!tracks.isArray() || tracks.size() != 1) return false;
        auto* track = tracks[0].getDynamicObject();
        if (track == nullptr) return false;
        auto* note = new juce::DynamicObject();
        note->setProperty("startTick", 0);
        note->setProperty("durationTicks", 120);
        note->setProperty("note", 60);
        note->setProperty("velocity", 100);
        note->setProperty("channel", 1);
        juce::Array<juce::var> notes;
        notes.add(juce::var(note));
        auto* midiClip = new juce::DynamicObject();
        midiClip->setProperty("startTick", 0);
        midiClip->setProperty("durationTicks", 960);
        midiClip->setProperty("loopEnabled", false);
        midiClip->setProperty("muted", false);
        midiClip->setProperty("notes", notes);
        midiClip->setProperty("events", juce::Array<juce::var>{});
        juce::Array<juce::var> midiClips;
        midiClips.add(juce::var(midiClip));
        track->setProperty("midiClips", midiClips);
        if (!engine.loadSnapshot(snapshot, formats, 48'000.0, 32, error)) return false;

        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                                48'000.0, 32, error);
        if (rack == nullptr || !rack->prepareTimelineMidiCapacity(2, error)) return false;
        if (!TimelineEngineTestPeer::installTrackInstrument(engine, "track:pdc", std::move(rack)))
            return false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.empty()) return false;
            engine.timeline->tracks.front()->runtime->compensationDelaySamples = 4;
        }
        if (!engine.enqueueTargetedMidi("track:pdc", juce::MidiMessage::noteOn(1, 72, 0.8f), error))
            return false;

        // Act
        engine.play();
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        const std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return trace.midiSamplePositions.size() >= 2 && trace.midiSamplePositions[0] == 0 &&
               trace.midiSamplePositions[1] == 4;
    }

    static bool panicClosesInstrumentRuntime() {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(makeInstrumentSnapshot("track:panic-instrument"), formats,
                                 48'000.0, 32, error))
            return false;

        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                                48'000.0, 32, error);
        if (rack == nullptr) return false;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.timeline == nullptr || engine.timeline->tracks.size() != 1) return false;
            auto& panicTrack = *engine.timeline->tracks.front();
            if (panicTrack.runtime == nullptr) return false;
            panicTrack.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.publishInProgress.store(true, std::memory_order_release);
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            engine.panicAllInstrumentTracks();
            engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
        }
        engine.publishInProgress.store(false, std::memory_order_release);
        engine.play();
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return trace.midiMessages.size() == 48u;
    }

    static bool audioDeviceRestartRebuildsRuntimeFormat() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        const auto snapshot = makeInstrumentSnapshot("track:audio-device");
        if (!engine.loadSnapshot(snapshot, formats, 48'000.0, 256, error) ||
            !engine.loadSnapshot(snapshot, formats, 48'000.0, 256, error, false) ||
            !engine.preparedTrackReusesRuntimeDevices("track:audio-device") ||
            !engine.commitPreparedSnapshot(error))
            return false;

        // Act
        engine.audioDeviceStarted();
        if (!engine.loadSnapshot(snapshot, formats, 44'100.0, 1024, error, false)) return false;

        double preparedSampleRate = 0.0;
        int preparedBlockSize = 0;
        double trackSampleRate = 0.0;
        bool reusesRuntimeDevices = true;
        {
            const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
            if (engine.pendingTimeline == nullptr || engine.pendingTimeline->tracks.empty())
                return false;
            preparedSampleRate = engine.pendingTimeline->outputSampleRate;
            preparedBlockSize = engine.pendingTimeline->preparedBlockSize;
            trackSampleRate = engine.pendingTimeline->tracks.front()->runtime->outputSampleRate;
            reusesRuntimeDevices = engine.pendingTimeline->tracks.front()->reuseRuntimeDevices;
        }
        if (reusesRuntimeDevices || std::abs(preparedSampleRate - 44'100.0) > 0.1 ||
            preparedBlockSize != 1024 || std::abs(trackSampleRate - 44'100.0) > 0.1)
            return false;

        // Assert
        return engine.commitPreparedSnapshot(error) &&
               std::abs(static_cast<double>(engine.status().getProperty("sampleRate", 0.0)) -
                        44'100.0) <= 0.1;
    }

    static juce::var run(const juce::File& directory) {
        auto* result = new juce::DynamicObject();
        result->setProperty("type", "timelineSelfTest");
        juce::Array<juce::var> checks;
        const auto mono = directory.getChildFile("timeline-44100-mono.wav");
        const auto stereo = directory.getChildFile("timeline-48000-stereo.wav");
        directory.createDirectory();
        const auto sourcesWritten = writePcmWave(mono, 44100, 1, 44100, 6000) &&
                                    writePcmWave(stereo, 48000, 2, 48000, 4000);

        bool loaded = false;
        bool mixed = false;
        bool seeked = false;
        bool looped = false;
        bool punchWindowed = false;
        bool immediateRecordStarted = false;
        bool countInAligned = false;
        bool countInAudible = false;
        bool countInCancelled = false;
        bool metronomeMixed = false;
        bool automationRamped = false;
        bool offlineRangeRendered = false;
        bool offlineAudioRendered = false;
        bool offlineNormalized = false;
        bool graphUpdateReusedDevices = false;
        bool mutablePluginStateKeepsTopology = false;
        bool recordingTapIsolated = false;
        bool loopCaptureSegments = false;
        bool syntheticLoopPassed = false;
        bool partialPassPassed = false;
        bool blockSizePassed = false;
        bool longRecordingPassed = false;
        bool productionWriterPassed = false;
        bool productionWriterPartialPassed = false;
        const auto liveInstrumentWhileStopped = liveInstrumentProcessesWhileStopped();
        const auto panicClosesRuntime = panicClosesInstrumentRuntime();
        int diagPartialSegments = 0;
        int diagPartialRaw = 0;
        int diagPartialProcessed = 0;
        int diagPartialWindowed = 0;
        int diagBsRaw = 0;
        int diagBsProcessed = 0;
        int diagBsWindowed = 0;
        int diagPartialFailIndex = -1;
        float diagPartialFailValue = 0.0f;
        float diagPartialRawAtFail = 0.0f;
        float automationEarlyLeft = 0.0f;
        float automationEarlyRight = 0.0f;
        float automationLateLeft = 0.0f;
        float automationLateRight = 0.0f;
        std::uint64_t diagProductionRawSamples = 0;
        std::uint64_t diagProductionProcessedSamples = 0;
        std::uint64_t diagProductionMissing = 0;
        std::uint64_t diagProductionDropped = 0;
        std::uint64_t diagProductionPartialRaw = 0;
        std::uint64_t diagProductionPartialProcessed = 0;
        juce::String error;
        {
            auto* first = new juce::DynamicObject();
            first->setProperty("id", "device:test");
            first->setProperty("kind", "plugin");
            first->setProperty("path", "C:\\test\\Effect.vst3");
            first->setProperty("bypassed", false);
            juce::Array<juce::var> firstParameters;
            firstParameters.add(0.1);
            firstParameters.add(0.2);
            first->setProperty("parameterValues", firstParameters);
            auto* second = new juce::DynamicObject();
            second->setProperty("id", "device:test");
            second->setProperty("kind", "plugin");
            second->setProperty("path", "C:\\test\\Effect.vst3");
            second->setProperty("bypassed", true);
            juce::Array<juce::var> secondParameters;
            secondParameters.add(0.8);
            secondParameters.add(0.9);
            second->setProperty("parameterValues", secondParameters);
            juce::Array<juce::var> firstChain;
            firstChain.add(juce::var(first));
            juce::Array<juce::var> secondChain;
            secondChain.add(juce::var(second));
            mutablePluginStateKeepsTopology = pluginTopologySignature(juce::var(firstChain)) ==
                                              pluginTopologySignature(juce::var(secondChain));
        }
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
            const auto addClip = [&clips](const juce::String& id, const juce::File& file,
                                          const int sampleRate, const int frames) {
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
                clip->setProperty("takeVariant", "raw");
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
            audioTrack->setProperty("armed", true);
            audioTrack->setProperty("monitoring", "off");
            auto* audioInput = new juce::DynamicObject();
            audioInput->setProperty("channelIndex", 0);
            audioTrack->setProperty("audioInput", juce::var(audioInput));
            auto* rack = new juce::DynamicObject();
            rack->setProperty("devices", juce::Array<juce::var>{});
            audioTrack->setProperty("rack", juce::var(rack));
            audioTrack->setProperty("audioClips", clips);
            audioTrack->setProperty("midiClips", juce::Array<juce::var>{});
            juce::Array<juce::var> automationPoints;
            const auto addAutomationPoint =
                [&automationPoints](const juce::String& id, const int tick, const double value) {
                    auto* point = new juce::DynamicObject();
                    point->setProperty("id", id);
                    point->setProperty("tick", tick);
                    point->setProperty("value", value);
                    automationPoints.add(juce::var(point));
                };
            addAutomationPoint("point:left", 0, -1.0);
            addAutomationPoint("point:right", 20, 1.0);
            auto* panAutomation = new juce::DynamicObject();
            panAutomation->setProperty("parameter", "pan");
            panAutomation->setProperty("points", automationPoints);
            juce::Array<juce::var> volumePoints;
            auto* quietPoint = new juce::DynamicObject();
            quietPoint->setProperty("id", "point:quiet");
            quietPoint->setProperty("tick", 0);
            quietPoint->setProperty("value", -24.0);
            volumePoints.add(juce::var(quietPoint));
            auto* loudPoint = new juce::DynamicObject();
            loudPoint->setProperty("id", "point:loud");
            loudPoint->setProperty("tick", 20);
            loudPoint->setProperty("value", 0.0);
            volumePoints.add(juce::var(loudPoint));
            auto* volumeAutomation = new juce::DynamicObject();
            volumeAutomation->setProperty("parameter", "volume");
            volumeAutomation->setProperty("points", volumePoints);
            juce::Array<juce::var> automation;
            automation.add(juce::var(volumeAutomation));
            automation.add(juce::var(panAutomation));
            audioTrack->setProperty("automation", automation);
            juce::Array<juce::var> tracks;
            tracks.add(juce::var(audioTrack));
            auto* placeholderTrack = new juce::DynamicObject();
            placeholderTrack->setProperty("id", "track:missing-instrument");
            placeholderTrack->setProperty("kind", "instrument");
            placeholderTrack->setProperty("gainDb", 0.0);
            placeholderTrack->setProperty("pan", 0.0);
            placeholderTrack->setProperty("muted", false);
            placeholderTrack->setProperty("solo", false);
            auto* placeholderInstrument = new juce::DynamicObject();
            placeholderInstrument->setProperty("id", "device:missing-instrument");
            placeholderInstrument->setProperty("type", "vst3");
            placeholderInstrument->setProperty("path", "C:\\missing\\Instrument.vst3");
            placeholderInstrument->setProperty("disabledPlaceholder", true);
            placeholderTrack->setProperty("instrument", juce::var(placeholderInstrument));
            auto* placeholderRack = new juce::DynamicObject();
            placeholderRack->setProperty("devices", juce::Array<juce::var>{});
            placeholderTrack->setProperty("rack", juce::var(placeholderRack));
            placeholderTrack->setProperty("audioClips", juce::Array<juce::var>{});
            placeholderTrack->setProperty("midiClips", juce::Array<juce::var>{});
            placeholderTrack->setProperty("automation", juce::Array<juce::var>{});
            tracks.add(juce::var(placeholderTrack));
            auto* snapshotObject = new juce::DynamicObject();
            snapshotObject->setProperty("revision", 7);
            snapshotObject->setProperty("timebase", juce::var(timebase));
            snapshotObject->setProperty("loopRange", juce::var(loopRange));
            snapshotObject->setProperty("tracks", tracks);
            const auto snapshot = juce::var(snapshotObject);
            loaded = engine.loadSnapshot(snapshot, formats, 48000.0, 512, error);
            if (loaded) {
                snapshotObject->setProperty("revision", 8);
                audioTrack->setProperty("gainDb", -3.0);
                graphUpdateReusedDevices =
                    engine.loadSnapshot(snapshot, formats, 48000.0, 512, error, false) &&
                    engine.preparedTrackReusesRuntimeDevices("track:test") &&
                    engine.commitPreparedSnapshot(error);
                OfflineRenderer offlineRenderer;
                OfflineRenderer::Result offlineResult;
                const auto offlineOutput = directory.getChildFile("offline-selection.wav");
                if (offlineRenderer.render(snapshot, formats, offlineOutput, 480, 1440, 48000.0,
                                           512, 0.0f, false, offlineResult, error)) {
                    auto reader = std::unique_ptr<juce::AudioFormatReader>(
                        formats.createReaderFor(offlineOutput));
                    juce::AudioBuffer<float> rendered(2, 24000);
                    offlineRangeRendered = reader != nullptr && reader->numChannels == 2 &&
                                           reader->lengthInSamples == 24000;
                    offlineAudioRendered = offlineRangeRendered &&
                                           reader->read(&rendered, 0, 24000, 0, true, true) &&
                                           std::max(rendered.getMagnitude(0, 0, 24000),
                                                    rendered.getMagnitude(1, 0, 24000)) > 0.01f;
                }
                OfflineRenderer::Result normalizedResult;
                const auto normalizedOutput = directory.getChildFile("offline-normalized.wav");
                if (offlineRenderer.render(snapshot, formats, normalizedOutput, 0, 1440, 48000.0,
                                           512, 0.0f, true, normalizedResult, error)) {
                    auto reader = std::unique_ptr<juce::AudioFormatReader>(
                        formats.createReaderFor(normalizedOutput));
                    if (reader != nullptr && reader->numChannels == 2 &&
                        reader->lengthInSamples > 0) {
                        const auto sampleCount = static_cast<int>(reader->lengthInSamples);
                        juce::AudioBuffer<float> rendered(2, sampleCount);
                        if (reader->read(&rendered, 0, sampleCount, 0, true, true)) {
                            const auto peak = std::max(rendered.getMagnitude(0, 0, sampleCount),
                                                       rendered.getMagnitude(1, 0, sampleCount));
                            offlineNormalized = std::abs(peak - 0.98f) <= 0.02f;
                        }
                    }
                }
                engine.seekToTick(0);
                engine.play();
                std::array<float, 512> left{};
                std::array<float, 512> right{};
                std::array<float*, 2> channels{left.data(), right.data()};
                engine.mix(channels.data(), 2, static_cast<int>(left.size()));
                automationEarlyLeft = std::abs(left[20]);
                automationEarlyRight = std::abs(right[20]);
                automationLateLeft = std::abs(left[490]);
                automationLateRight = std::abs(right[490]);
                automationRamped = std::abs(left[20]) > std::abs(right[20]) * 2.0f &&
                                   std::abs(right[490]) > std::abs(left[490]) * 2.0f &&
                                   std::abs(left[490]) + std::abs(right[490]) >
                                       (std::abs(left[20]) + std::abs(right[20])) * 4.0f;
                for (int block = 1; block < 8; ++block)
                    engine.mix(channels.data(), 2, static_cast<int>(left.size()));
                const auto peak = std::max(*std::max_element(left.begin(), left.end()),
                                           *std::max_element(right.begin(), right.end()));
                mixed = peak > 0.1f;
                engine.seekToTick(960);
                const auto seekStatus = engine.status();
                seeked =
                    static_cast<juce::int64>(seekStatus.getProperty("timelineSample", -1)) == 24000;

                CaptureIsolationSink captureSink(directory);
                engine.setRecordingSink(&captureSink);
                engine.seekToTick(0);
                int captureOffset = 0;
                int captureSamples = 0;
                std::array<float, 512> physicalInput{};
                physicalInput.fill(0.05f);
                std::array<float, 512> captureLeft{};
                std::array<float, 512> captureRight{};
                const std::array<const float*, 1> physicalInputs{physicalInput.data()};
                const std::array<float*, 2> captureOutputs{captureLeft.data(), captureRight.data()};
                const auto captureStarted = engine.startRecording(0, error);
                const auto captureWindow =
                    captureStarted && engine.recordingWindow(static_cast<int>(physicalInput.size()),
                                                             captureOffset, captureSamples);
                if (captureWindow)
                    engine.mix(physicalInputs.data(), 1, captureOutputs.data(), 2,
                               static_cast<int>(physicalInput.size()));
                engine.stopRecording();
                engine.stop();
                engine.clearRecordingSink();
                recordingTapIsolated =
                    captureWindow && captureOffset == 0 &&
                    captureSamples == static_cast<int>(physicalInput.size()) &&
                    captureSink.receivedTrack == "track:test" &&
                    captureSink.receivedSamples == static_cast<int>(physicalInput.size()) &&
                    captureSink.isolated;

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
                loopSnapshot->setProperty("metronomeEnabled", true);
                auto* loopTrack = new juce::DynamicObject();
                loopTrack->setProperty("id", "track:loop");
                loopTrack->setProperty("gainDb", 0.0);
                loopTrack->setProperty("pan", 0.0);
                loopTrack->setProperty("muted", false);
                loopTrack->setProperty("solo", false);
                loopTrack->setProperty("armed", true);
                loopTrack->setProperty("monitoring", "off");
                auto* loopInput = new juce::DynamicObject();
                loopInput->setProperty("channelIndex", 0);
                loopTrack->setProperty("audioInput", juce::var(loopInput));
                auto* loopRack = new juce::DynamicObject();
                loopRack->setProperty("devices", juce::Array<juce::var>{});
                loopTrack->setProperty("rack", juce::var(loopRack));
                loopTrack->setProperty("audioClips", juce::Array<juce::var>{});
                loopTrack->setProperty("midiClips", juce::Array<juce::var>{});
                juce::Array<juce::var> loopTracks;
                loopTracks.add(juce::var(loopTrack));
                loopSnapshot->setProperty("tracks", loopTracks);
                const auto loopSnapshotValue = juce::var(loopSnapshot);
                auto punchSnapshot =
                    juce::JSON::parse(juce::JSON::toString(loopSnapshotValue, false));
                punchSnapshot.getDynamicObject()->setProperty("punchRange", juce::var(punchRange));
                const auto loopSnapshotLoaded =
                    engine.loadSnapshot(loopSnapshotValue, formats, 48000.0, 512, error);
                if (loopSnapshotLoaded) {
                    CaptureIsolationSink loopCaptureSink(directory);
                    engine.setRecordingSink(&loopCaptureSink);
                    engine.seekToTick(0);
                    int loopCaptureOffset = 0;
                    int loopCaptureSamples = 0;
                    constexpr int loopPassSamples = 24'000;
                    constexpr int loopBlockSamples = 512;
                    constexpr int loopTotalSamples = loopPassSamples * 3;
                    std::array<float, loopBlockSamples> loopAudioInput{};
                    loopAudioInput.fill(0.05f);
                    std::array<float, loopBlockSamples> loopOutputLeft{};
                    std::array<float, loopBlockSamples> loopOutputRight{};
                    const std::array<const float*, 1> loopInputs{loopAudioInput.data()};
                    const std::array<float*, 2> loopOutputs{loopOutputLeft.data(),
                                                            loopOutputRight.data()};
                    const auto loopRecordingStarted = engine.startRecording(0, error);
                    const auto loopWindowed =
                        loopRecordingStarted &&
                        engine.recordingWindow(loopTotalSamples, loopCaptureOffset,
                                               loopCaptureSamples);
                    auto loopRemaining = loopTotalSamples;
                    while (loopWindowed && loopRemaining > loopBlockSamples) {
                        engine.mix(loopInputs.data(), 1, loopOutputs.data(), 2, loopBlockSamples);
                        loopRemaining -= loopBlockSamples;
                    }
                    if (loopWindowed && loopRemaining > 0)
                        engine.mix(loopInputs.data(), 1, loopOutputs.data(), 2, loopRemaining);
                    engine.stopRecording();
                    engine.flushRecordingTail(error);
                    engine.stop();
                    engine.clearRecordingSink();
                    loopCaptureSegments =
                        loopWindowed && loopCaptureOffset == 0 &&
                        loopCaptureSamples == loopTotalSamples && loopCaptureSink.beginCount == 3 &&
                        loopCaptureSink.endCount == 3 && loopCaptureSink.loopBoundaryCount == 3 &&
                        loopCaptureSink.totalRawSamples == loopTotalSamples &&
                        loopCaptureSink.totalProcessedSamples == loopTotalSamples &&
                        loopCaptureSink.offlineProcessedWriteCalls > 1 &&
                        loopCaptureSink.maxOfflineProcessedWriteSize <= loopBlockSamples;
                    for (int index = 0; loopCaptureSegments && index < 3; ++index) {
                        const auto offset = static_cast<std::size_t>(index);
                        loopCaptureSegments =
                            loopCaptureSink.segmentRawSamples[offset] > 0 &&
                            loopCaptureSink.endAudioSamples[offset] >
                                loopCaptureSink.beginAudioSamples[offset] &&
                            (index == 0 || loopCaptureSink.beginAudioSamples[offset] >=
                                               loopCaptureSink.endAudioSamples[offset - 1]) &&
                            loopCaptureSink.beginTimelineSamples[offset] == 0 &&
                            loopCaptureSink.endTimelineSamples[offset] ==
                                static_cast<std::uint64_t>(loopPassSamples);
                    }
                }
                // Synthetic Plugin Loop Recording test:
                // Latency 256, Tail 48000, Loop 24000, 3 passes with distinct impulses
                if (loopSnapshotLoaded &&
                    engine.loadSnapshot(loopSnapshotValue, formats, 48000.0, 512, error)) {
                    constexpr int kSynthDelay = 256;
                    constexpr int kSynthTail = 48'000;
                    constexpr int kSynthLoopLength = 24'000;
                    constexpr int kSynthPasses = 3;
                    constexpr int kSynthTotal = kSynthLoopLength * kSynthPasses;
                    constexpr int kSynthBlock = 512;
                    constexpr float kImpulseAmplitude = 0.9f;
                    // Impulse positions within each pass (must be >= kSynthDelay)
                    constexpr int kImpulsePos[kSynthPasses] = {256, 1256, 2256};
                    // Set synthetic plugin delay and tail on the armed track
                    {
                        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                        if (engine.timeline != nullptr) {
                            for (auto& trackPtr : engine.timeline->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack &&
                                    trackPtr->runtime->armed) {
                                    trackPtr->runtime->pluginDelaySamples = kSynthDelay;
                                    trackPtr->runtime->pluginTailSamples = kSynthTail;
                                }
                            }
                        }
                    }
                    LoopDataCaptureSink synthSink(directory, "synth");
                    engine.setRecordingSink(&synthSink);
                    engine.seekToTick(0);
                    int synthOffset = 0;
                    int synthSamples = 0;
                    const auto synthStarted = engine.startRecording(0, error);
                    const auto synthWindowed =
                        synthStarted &&
                        engine.recordingWindow(kSynthTotal, synthOffset, synthSamples);
                    const auto clockBefore =
                        engine.audioClockSample.load(std::memory_order_acquire);
                    std::array<float, kSynthBlock> synthInput{};
                    std::array<float, kSynthBlock> synthOutL{};
                    std::array<float, kSynthBlock> synthOutR{};
                    const std::array<const float*, 1> synthInputs{synthInput.data()};
                    const std::array<float*, 2> synthOutputs{synthOutL.data(), synthOutR.data()};
                    int synthMixed = 0;
                    bool synthClockContinuous = true;
                    while (synthWindowed && synthMixed < kSynthTotal) {
                        const auto block = std::min(kSynthBlock, kSynthTotal - synthMixed);
                        const auto passIndex = synthMixed / kSynthLoopLength;
                        const auto posInPass = synthMixed % kSynthLoopLength;
                        synthInput.fill(0.0f);
                        if (passIndex < kSynthPasses && posInPass <= kImpulsePos[passIndex] &&
                            kImpulsePos[passIndex] < posInPass + block) {
                            synthInput[static_cast<std::size_t>(kImpulsePos[passIndex] -
                                                                posInPass)] = kImpulseAmplitude;
                        }
                        const auto prevClock =
                            engine.audioClockSample.load(std::memory_order_acquire);
                        engine.mix(synthInputs.data(), 1, synthOutputs.data(), 2, block);
                        const auto newClock =
                            engine.audioClockSample.load(std::memory_order_acquire);
                        if (newClock != prevClock + static_cast<std::uint64_t>(block))
                            synthClockContinuous = false;
                        synthMixed += block;
                    }
                    engine.stopRecording();
                    engine.flushRecordingTail(error);
                    engine.stop();
                    engine.clearRecordingSink();
                    const auto clockAfter = engine.audioClockSample.load(std::memory_order_acquire);
                    // Verify: audio clock advanced continuously by total mixed samples
                    const bool synthClockOk =
                        synthClockContinuous &&
                        clockAfter == clockBefore + static_cast<std::uint64_t>(synthMixed);
                    // Verify: timeline wrapped (position < loopLength after stop)
                    const auto finalPosition =
                        engine.timelineSample.load(std::memory_order_acquire);
                    const bool synthTimelineWrapped =
                        finalPosition >= 0 && finalPosition < kSynthLoopLength;
                    // Verify: 3 segments, 3 boundaries
                    const bool synthSegmentsOk = synthSink.segmentCount == kSynthPasses &&
                                                 synthSink.boundaryCount == kSynthPasses;
                    // Verify: raw and processed lengths match
                    const bool synthLengthsOk =
                        synthSink.totalRaw == kSynthTotal &&
                        synthSink.totalProcessed == kSynthTotal &&
                        static_cast<int>(synthSink.rawBuffer.size()) == kSynthTotal &&
                        static_cast<int>(synthSink.processedLeft.size()) == kSynthTotal;
                    // Verify: impulse positions and no cross-pass contamination
                    bool synthImpulseOk = true;
                    bool synthNoCrossPass = true;
                    for (int pass = 0; synthImpulseOk && pass < kSynthPasses; ++pass) {
                        const auto base = static_cast<std::size_t>(pass * kSynthLoopLength);
                        const auto impulseAt = static_cast<std::size_t>(kImpulsePos[pass]);
                        // Raw impulse present at position P
                        synthImpulseOk = base + impulseAt < synthSink.rawBuffer.size() &&
                                         std::abs(synthSink.rawBuffer[base + impulseAt] -
                                                  kImpulseAmplitude) < 0.001f;
                        // Processed impulse at P - delay (passthrough chain + delay compensation)
                        // For a real plugin with latency D, output aligns at P.
                        // For passthrough test chain, compensation shifts left by D.
                        const auto processedPos = impulseAt - static_cast<std::size_t>(kSynthDelay);
                        synthImpulseOk = synthImpulseOk &&
                                         base + processedPos < synthSink.processedLeft.size() &&
                                         std::abs(synthSink.processedLeft[base + processedPos] -
                                                  kImpulseAmplitude) < 0.001f;
                        // No other significant samples in this pass (no contamination)
                        for (int i = 0; synthNoCrossPass && i < kSynthLoopLength; ++i) {
                            if (static_cast<std::size_t>(i) == processedPos) continue;
                            const auto idx = base + static_cast<std::size_t>(i);
                            if (idx < synthSink.processedLeft.size() &&
                                std::abs(synthSink.processedLeft[idx]) > 0.001f)
                                synthNoCrossPass = false;
                        }
                    }
                    syntheticLoopPassed = synthWindowed && synthClockOk && synthTimelineWrapped &&
                                          synthSegmentsOk && synthLengthsOk && synthImpulseOk &&
                                          synthNoCrossPass &&
                                          synthSink.offlineProcessedWriteCalls > 1 &&
                                          synthSink.maxOfflineProcessedWriteSize <= kSynthBlock;
                }
                // Partial Pass test: start recording from loop middle
                if (loopSnapshotLoaded &&
                    engine.loadSnapshot(loopSnapshotValue, formats, 48000.0, 512, error)) {
                    constexpr int kPartialTotal = 60'000;  // 12000 + 24000 + 24000
                    constexpr int kPartialBlock = 512;
                    // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                    {
                        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                        if (engine.timeline != nullptr) {
                            for (auto& trackPtr : engine.timeline->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack &&
                                    trackPtr->runtime->armed) {
                                    trackPtr->runtime->pluginDelaySamples = 0;
                                    trackPtr->runtime->pluginTailSamples = 0;
                                }
                            }
                        }
                    }
                    LoopDataCaptureSink partialSink(directory, "partial");
                    engine.setRecordingSink(&partialSink);
                    engine.seekToTick(480);  // tick 480 = sample 12000
                    int partialOffset = 0;
                    int partialSamples = 0;
                    const auto partialStarted = engine.startRecording(0, error);
                    const auto partialWindowed =
                        partialStarted &&
                        engine.recordingWindow(kPartialTotal, partialOffset, partialSamples);
                    std::array<float, kPartialBlock> partialInput{};
                    partialInput.fill(0.05f);
                    std::array<float, kPartialBlock> partialOutL{};
                    std::array<float, kPartialBlock> partialOutR{};
                    const std::array<const float*, 1> partialInputs{partialInput.data()};
                    const std::array<float*, 2> partialOutputs{partialOutL.data(),
                                                               partialOutR.data()};
                    int partialMixed = 0;
                    while (partialWindowed && partialMixed < kPartialTotal) {
                        const auto block = std::min(kPartialBlock, kPartialTotal - partialMixed);
                        engine.mix(partialInputs.data(), 1, partialOutputs.data(), 2, block);
                        partialMixed += block;
                    }
                    engine.stopRecording();
                    engine.flushRecordingTail(error);
                    engine.stop();
                    engine.clearRecordingSink();
                    // Expected: 3 segments (partial 12000, full 24000, full 24000)
                    const bool partialSegmentsOk = partialSink.segmentCount == 3;
                    const bool partialLengthsOk =
                        partialSink.totalRaw == kPartialTotal &&
                        partialSink.totalProcessed == kPartialTotal &&
                        static_cast<int>(partialSink.processedLeft.size()) == kPartialTotal;
                    // Each segment independently reset (no cross-segment contamination)
                    // With constant 0.05 input and passthrough chain, all processed ≈ 0.05
                    bool partialDataOk =
                        partialSink.processedLeft.size() == static_cast<std::size_t>(kPartialTotal);
                    int partialFailIndex = -1;
                    for (int i = 0; partialDataOk && i < kPartialTotal; ++i) {
                        if (std::abs(partialSink.processedLeft[static_cast<std::size_t>(i)] -
                                     0.05f) >= 0.02f) {
                            partialDataOk = false;
                            partialFailIndex = i;
                        }
                    }
                    partialPassPassed = partialWindowed && partialSegmentsOk && partialLengthsOk &&
                                        partialDataOk &&
                                        partialSink.offlineProcessedWriteCalls > 1 &&
                                        partialSink.maxOfflineProcessedWriteSize <= kPartialBlock;
                    diagPartialSegments = partialSink.segmentCount;
                    diagPartialRaw = partialSink.totalRaw;
                    diagPartialProcessed = partialSink.totalProcessed;
                    diagPartialWindowed = partialWindowed ? 1 : 0;
                    diagPartialFailIndex = partialFailIndex;
                    if (partialFailIndex >= 0 && static_cast<std::size_t>(partialFailIndex) <
                                                     partialSink.processedLeft.size())
                        diagPartialFailValue =
                            partialSink.processedLeft[static_cast<std::size_t>(partialFailIndex)];
                    if (partialFailIndex >= 0 &&
                        static_cast<std::size_t>(partialFailIndex) < partialSink.rawBuffer.size())
                        diagPartialRawAtFail =
                            partialSink.rawBuffer[static_cast<std::size_t>(partialFailIndex)];
                }
                // Block Size test: verify processing uses small blocks
                if (loopSnapshotLoaded &&
                    engine.loadSnapshot(loopSnapshotValue, formats, 48000.0, 128, error)) {
                    // preparedBlockSize = 128; chain must be fed in <= 128 sample chunks
                    constexpr int kBsTotal = 24'000;  // 1 pass
                    constexpr int kBsBlock = 128;
                    constexpr int kBsDelay = 64;
                    constexpr float kBsImpulse = 0.8f;
                    constexpr int kBsImpulsePos = 500;
                    {
                        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                        if (engine.timeline != nullptr) {
                            for (auto& trackPtr : engine.timeline->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack && trackPtr->runtime->armed)
                                    trackPtr->runtime->pluginDelaySamples = kBsDelay;
                            }
                        }
                    }
                    LoopDataCaptureSink bsSink(directory, "blocksize");
                    engine.setRecordingSink(&bsSink);
                    engine.seekToTick(0);
                    int bsOffset = 0;
                    int bsSamples = 0;
                    const auto bsStarted = engine.startRecording(0, error);
                    const auto bsWindowed =
                        bsStarted && engine.recordingWindow(kBsTotal, bsOffset, bsSamples);
                    std::array<float, kBsBlock> bsInput{};
                    std::array<float, kBsBlock> bsOutL{};
                    std::array<float, kBsBlock> bsOutR{};
                    const std::array<const float*, 1> bsInputs{bsInput.data()};
                    const std::array<float*, 2> bsOutputs{bsOutL.data(), bsOutR.data()};
                    int bsMixed = 0;
                    while (bsWindowed && bsMixed < kBsTotal) {
                        bsInput.fill(0.0f);
                        if (bsMixed <= kBsImpulsePos && kBsImpulsePos < bsMixed + kBsBlock) {
                            bsInput[static_cast<std::size_t>(kBsImpulsePos - bsMixed)] = kBsImpulse;
                        }
                        engine.mix(bsInputs.data(), 1, bsOutputs.data(), 2, kBsBlock);
                        bsMixed += kBsBlock;
                    }
                    engine.stopRecording();
                    engine.flushRecordingTail(error);
                    engine.stop();
                    engine.clearRecordingSink();
                    // Verify: processed length matches raw, impulse at correct position
                    const bool bsLengthOk =
                        bsSink.totalRaw == kBsTotal && bsSink.totalProcessed == kBsTotal;
                    const auto bsProcessedPos = kBsImpulsePos - kBsDelay;
                    const bool bsImpulseOk =
                        bsProcessedPos >= 0 &&
                        static_cast<std::size_t>(bsProcessedPos) < bsSink.processedLeft.size() &&
                        std::abs(bsSink.processedLeft[static_cast<std::size_t>(bsProcessedPos)] -
                                 kBsImpulse) < 0.01f;
                    blockSizePassed = bsWindowed && bsLengthOk && bsImpulseOk &&
                                      bsSink.offlineProcessedWriteCalls > 1 &&
                                      bsSink.maxOfflineProcessedWriteSize <= kBsBlock;
                    diagBsRaw = bsSink.totalRaw;
                    diagBsProcessed = bsSink.totalProcessed;
                    diagBsWindowed = bsWindowed ? 1 : 0;
                }
                // Long Recording test: 130 passes exceeds old 128x RAM limit
                if (loopSnapshotLoaded &&
                    engine.loadSnapshot(loopSnapshotValue, formats, 48000.0, 512, error)) {
                    constexpr int kLongLoopLength = 4'800;  // short loop
                    constexpr int kLongPasses = 130;
                    constexpr int kLongTotal = kLongLoopLength * kLongPasses;
                    constexpr int kLongBlock = 512;
                    // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                    {
                        const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                        if (engine.timeline != nullptr) {
                            for (auto& trackPtr : engine.timeline->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack &&
                                    trackPtr->runtime->armed) {
                                    trackPtr->runtime->pluginDelaySamples = 0;
                                    trackPtr->runtime->pluginTailSamples = 0;
                                }
                            }
                        }
                    }
                    // Use a snapshot with short loop (tick 0..192 = 4800 samples)
                    auto* longLoop = new juce::DynamicObject();
                    longLoop->setProperty("enabled", true);
                    longLoop->setProperty("startTick", 0);
                    longLoop->setProperty("endTick", 192);
                    loopSnapshot->setProperty("loopRange", juce::var(longLoop));
                    const auto longSnapshotValue = juce::var(loopSnapshot);
                    if (engine.loadSnapshot(longSnapshotValue, formats, 48000.0, 512, error)) {
                        LoopDataCaptureSink longSink(directory, "longrec");
                        engine.setRecordingSink(&longSink);
                        engine.seekToTick(0);
                        int longOffset = 0;
                        int longSamples = 0;
                        const auto longStarted = engine.startRecording(0, error);
                        const auto longWindowed =
                            longStarted &&
                            engine.recordingWindow(kLongTotal, longOffset, longSamples);
                        std::array<float, kLongBlock> longInput{};
                        longInput.fill(0.02f);
                        std::array<float, kLongBlock> longOutL{};
                        std::array<float, kLongBlock> longOutR{};
                        const std::array<const float*, 1> longInputs{longInput.data()};
                        const std::array<float*, 2> longOutputs{longOutL.data(), longOutR.data()};
                        int longMixed = 0;
                        while (longWindowed && longMixed < kLongTotal) {
                            const auto block = std::min(kLongBlock, kLongTotal - longMixed);
                            engine.mix(longInputs.data(), 1, longOutputs.data(), 2, block);
                            longMixed += block;
                        }
                        engine.stopRecording();
                        engine.flushRecordingTail(error);
                        engine.stop();
                        engine.clearRecordingSink();
                        // Verify: all 130 passes recorded, raw/processed match
                        longRecordingPassed =
                            longWindowed && longSink.segmentCount == kLongPasses &&
                            longSink.totalRaw == kLongTotal &&
                            longSink.totalProcessed == kLongTotal &&
                            static_cast<int>(longSink.processedLeft.size()) == kLongTotal &&
                            longSink.offlineProcessedWriteCalls > 1 &&
                            longSink.maxOfflineProcessedWriteSize <= kLongBlock;
                    }
                    // Restore original loop range for subsequent tests
                    auto* restoreLoop = new juce::DynamicObject();
                    restoreLoop->setProperty("enabled", true);
                    restoreLoop->setProperty("startTick", 0);
                    restoreLoop->setProperty("endTick", 960);
                    loopSnapshot->setProperty("loopRange", juce::var(restoreLoop));
                }
                // Production Writer Integration Test: 4 bars x 3 passes (384,000 x 3)
                {
                    auto* prodTimebase = new juce::DynamicObject();
                    prodTimebase->setProperty("ppq", 960);
                    prodTimebase->setProperty("bpm", 120.0);
                    prodTimebase->setProperty("timeSignatureNumerator", 4);
                    prodTimebase->setProperty("timeSignatureDenominator", 4);
                    auto* prodLoop = new juce::DynamicObject();
                    prodLoop->setProperty("enabled", true);
                    prodLoop->setProperty("startTick", 0);
                    prodLoop->setProperty("endTick", 15360);
                    auto* prodTrack = new juce::DynamicObject();
                    prodTrack->setProperty("id", "track:prod");
                    prodTrack->setProperty("gainDb", 0.0);
                    prodTrack->setProperty("pan", 0.0);
                    prodTrack->setProperty("muted", false);
                    prodTrack->setProperty("solo", false);
                    prodTrack->setProperty("armed", true);
                    prodTrack->setProperty("monitoring", "off");
                    auto* prodInput = new juce::DynamicObject();
                    prodInput->setProperty("channelIndex", 0);
                    prodTrack->setProperty("audioInput", juce::var(prodInput));
                    auto* prodRack = new juce::DynamicObject();
                    prodRack->setProperty("devices", juce::Array<juce::var>{});
                    prodTrack->setProperty("rack", juce::var(prodRack));
                    prodTrack->setProperty("audioClips", juce::Array<juce::var>{});
                    prodTrack->setProperty("midiClips", juce::Array<juce::var>{});
                    juce::Array<juce::var> prodTracks;
                    prodTracks.add(juce::var(prodTrack));
                    auto* prodSnapshot = new juce::DynamicObject();
                    prodSnapshot->setProperty("revision", 10);
                    prodSnapshot->setProperty("timebase", juce::var(prodTimebase));
                    prodSnapshot->setProperty("loopRange", juce::var(prodLoop));
                    prodSnapshot->setProperty("tracks", prodTracks);
                    const auto prodSnapshotValue = juce::var(prodSnapshot);

                    if (engine.loadSnapshot(prodSnapshotValue, formats, 48000.0, 512, error)) {
                        constexpr int kProdLoopLength = 384'000;
                        constexpr int kProdPasses = 3;
                        constexpr int kProdTotal = kProdLoopLength * kProdPasses;
                        constexpr int kProdBlock = 512;

                        // 3 full passes
                        // SafetyAudioCallback owns stop, tail flush, sink clear and session finish.
                        engine.seekToTick(0);
                        auto prodDir = directory.getChildFile("prod-writer");
                        SafetyAudioCallback prodCallback;
                        prodCallback.setTimelineEngine(&engine);
                        juce::String sessionError;
                        const auto prodArrangeStarted =
                            prodCallback.startArrangeRecording(prodDir, engine, sessionError);
                        if (prodArrangeStarted) {
                            int prodOffset = 0;
                            int prodSamples = 0;
                            const auto prodStarted = engine.startRecording(0, error);
                            const auto prodWindowed =
                                prodStarted &&
                                engine.recordingWindow(kProdTotal, prodOffset, prodSamples);
                            std::array<float, kProdBlock> prodIn{};
                            prodIn.fill(0.06f);
                            std::array<float, kProdBlock> prodOutL{};
                            std::array<float, kProdBlock> prodOutR{};
                            const std::array<const float*, 1> prodInputs{prodIn.data()};
                            const std::array<float*, 2> prodOutputs{prodOutL.data(),
                                                                    prodOutR.data()};
                            int prodMixed = 0;
                            while (prodWindowed && prodMixed < kProdTotal) {
                                const auto block = std::min(kProdBlock, kProdTotal - prodMixed);
                                engine.mix(prodInputs.data(), 1, prodOutputs.data(), 2, block);
                                prodMixed += block;
                                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                            }
                            const auto preStopStatus = prodCallback.recordingStatus();
                            juce::String stopError;
                            const auto stopOk =
                                prodCallback.stopArrangeRecording(engine, stopError);
                            engine.stop();

                            const auto rawFile = prodDir.getChildFile("tracks/0000/raw.wav");
                            const auto processedFile =
                                prodDir.getChildFile("tracks/0000/processed.wav");
                            const auto manifestFile = prodDir.getChildFile("manifest.json");
                            const auto manifestValue =
                                juce::JSON::parse(manifestFile.loadFileAsString());
                            auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(rawFile));
                            auto processedReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                            const auto rawLength =
                                rawReader != nullptr ? rawReader->lengthInSamples : 0;
                            const auto processedLength =
                                processedReader != nullptr ? processedReader->lengthInSamples : 0;
                            const auto completed =
                                manifestValue.isObject() &&
                                manifestValue.getProperty("state", {}).toString() == "completed";

                            diagProductionRawSamples = static_cast<std::uint64_t>(rawLength);
                            diagProductionProcessedSamples =
                                static_cast<std::uint64_t>(processedLength);
                            if (preStopStatus.isObject()) {
                                diagProductionMissing =
                                    static_cast<std::uint64_t>(static_cast<juce::int64>(
                                        preStopStatus.getProperty("processedMissingSamples", 0)));
                                diagProductionDropped =
                                    static_cast<std::uint64_t>(static_cast<juce::int64>(
                                        preStopStatus.getProperty("droppedBlocks", 0)));
                            }

                            productionWriterPassed =
                                prodWindowed && stopOk && rawFile.existsAsFile() &&
                                processedFile.existsAsFile() && rawLength == kProdTotal &&
                                processedLength == kProdTotal && completed &&
                                diagProductionMissing == 0 && diagProductionDropped == 0;

                            // Verify capture segment ranges match
                            if (productionWriterPassed && manifestValue.isObject()) {
                                const auto manifestTracks = manifestValue.getProperty("tracks", {});
                                if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                    const auto trackObj = manifestTracks[0];
                                    const auto segments =
                                        trackObj.getProperty("captureSegments", {});
                                    if (segments.isArray() && segments.size() == kProdPasses) {
                                        for (int i = 0; i < kProdPasses; ++i) {
                                            const auto seg = segments[i];
                                            const auto rawStart = static_cast<juce::int64>(
                                                seg.getProperty("rawFileStartSample", -1));
                                            const auto rawEnd = static_cast<juce::int64>(
                                                seg.getProperty("rawFileEndSample", -1));
                                            const auto procStart = static_cast<juce::int64>(
                                                seg.getProperty("processedFileStartSample", -1));
                                            const auto procEnd = static_cast<juce::int64>(
                                                seg.getProperty("processedFileEndSample", -1));
                                            if (rawStart != procStart || rawEnd != procEnd) {
                                                productionWriterPassed = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        productionWriterPassed = false;
                                    }
                                } else {
                                    productionWriterPassed = false;
                                }
                            }
                        }

                        // Partial pass: start mid-loop, record partial+full+partial
                        engine.seekToTick(7680);
                        auto partialConfig = engine.recordingConfiguration();
                        juce::String partialSessionError;
                        auto partialDir = directory.getChildFile("prod-writer-partial");
                        auto partialSession = ArrangeRecordingSession::create(
                            partialDir, partialConfig, partialSessionError);
                        if (partialSession != nullptr) {
                            engine.setRecordingSink(partialSession.get());
                            constexpr int kPartialTotal = 768'000;
                            int partialOffset = 0;
                            int partialSamples = 0;
                            const auto partialStarted = engine.startRecording(0, error);
                            const auto partialWindowed =
                                partialStarted && engine.recordingWindow(
                                                      kPartialTotal, partialOffset, partialSamples);
                            std::array<float, kProdBlock> partIn{};
                            partIn.fill(0.06f);
                            std::array<float, kProdBlock> partOutL{};
                            std::array<float, kProdBlock> partOutR{};
                            const std::array<const float*, 1> partInputs{partIn.data()};
                            const std::array<float*, 2> partOutputs{partOutL.data(),
                                                                    partOutR.data()};
                            int partMixed = 0;
                            while (partialWindowed && partMixed < kPartialTotal) {
                                const auto block = std::min(kProdBlock, kPartialTotal - partMixed);
                                engine.mix(partInputs.data(), 1, partOutputs.data(), 2, block);
                                partMixed += block;
                                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                            }
                            engine.stopRecording();
                            const auto tailOk = engine.flushRecordingTail(error);
                            engine.stop();
                            engine.clearRecordingSink();
                            juce::String finishError;
                            const auto finished = partialSession->finish(finishError);

                            const auto rawFile = partialDir.getChildFile("tracks/0000/raw.wav");
                            const auto processedFile =
                                partialDir.getChildFile("tracks/0000/processed.wav");
                            const auto manifestFile = partialDir.getChildFile("manifest.json");
                            const auto manifestValue =
                                juce::JSON::parse(manifestFile.loadFileAsString());
                            auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(rawFile));
                            auto processedReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                            const auto rawLength =
                                rawReader != nullptr ? rawReader->lengthInSamples : 0;
                            const auto processedLength =
                                processedReader != nullptr ? processedReader->lengthInSamples : 0;
                            const auto completed =
                                manifestValue.isObject() &&
                                manifestValue.getProperty("state", {}).toString() == "completed";

                            diagProductionPartialRaw = static_cast<std::uint64_t>(rawLength);
                            diagProductionPartialProcessed =
                                static_cast<std::uint64_t>(processedLength);

                            productionWriterPartialPassed =
                                partialWindowed && tailOk && finished && rawFile.existsAsFile() &&
                                processedFile.existsAsFile() && rawLength == kPartialTotal &&
                                processedLength == kPartialTotal && completed;

                            // Verify 3 segments and raw/processed ranges
                            if (productionWriterPartialPassed && manifestValue.isObject()) {
                                const auto manifestTracks = manifestValue.getProperty("tracks", {});
                                if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                    const auto trackObj = manifestTracks[0];
                                    const auto segments =
                                        trackObj.getProperty("captureSegments", {});
                                    if (segments.isArray() && segments.size() == 3) {
                                        for (int i = 0; i < 3; ++i) {
                                            const auto seg = segments[i];
                                            const auto rawStart = static_cast<juce::int64>(
                                                seg.getProperty("rawFileStartSample", -1));
                                            const auto rawEnd = static_cast<juce::int64>(
                                                seg.getProperty("rawFileEndSample", -1));
                                            const auto procStart = static_cast<juce::int64>(
                                                seg.getProperty("processedFileStartSample", -1));
                                            const auto procEnd = static_cast<juce::int64>(
                                                seg.getProperty("processedFileEndSample", -1));
                                            if (rawStart != procStart || rawEnd != procEnd) {
                                                productionWriterPartialPassed = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        productionWriterPartialPassed = false;
                                    }
                                } else {
                                    productionWriterPartialPassed = false;
                                }
                            }
                        }
                    }
                }
                if (loopSnapshotLoaded &&
                    engine.loadSnapshot(punchSnapshot, formats, 48000.0, 512, error)) {
                    int punchOffset = 0;
                    int punchSamples = 0;
                    engine.seekToTick(480);
                    error.clear();
                    const auto punchStarted = engine.startRecording(0, error);
                    punchWindowed = punchStarted &&
                                    engine.recordingWindow(512, punchOffset, punchSamples) &&
                                    punchOffset == 0 && punchSamples == 512;
                    if (!punchStarted && error.isEmpty())
                        error = "Punch self-test could not start Arrange recording.";
                    engine.stopRecording();
                    engine.stop();
                    engine.seekToTick(480);
                    if (engine.startRecording(0, error)) {
                        int immediateOffset = 0;
                        int immediateSamples = 0;
                        std::array<float, 512> immediateOutput{};
                        std::array<float*, 1> immediateChannels{immediateOutput.data()};
                        const auto immediateWindow =
                            engine.recordingWindow(static_cast<int>(immediateOutput.size()),
                                                   immediateOffset, immediateSamples);
                        engine.mix(immediateChannels.data(), 1,
                                   static_cast<int>(immediateOutput.size()));
                        const auto immediateStatus = engine.status();
                        immediateRecordStarted =
                            immediateWindow && immediateOffset == 0 && immediateSamples == 512 &&
                            immediateStatus.getProperty("state", {}).toString() == "playing" &&
                            static_cast<juce::int64>(
                                immediateStatus.getProperty("timelineSample", -1)) == 12'512;
                        engine.stopRecording();
                        engine.stop();
                        engine.seekToTick(480);
                    }
                    if (engine.startRecording(1, error)) {
                        int countInOffset = 0;
                        int countInSamples = 0;
                        constexpr int countInBlockSamples = 24'128;
                        const auto countInWindow = engine.recordingWindow(
                            countInBlockSamples, countInOffset, countInSamples);
                        std::vector<float> countInOutput(countInBlockSamples);
                        std::array<float*, 1> countInChannels{countInOutput.data()};
                        engine.mix(countInChannels.data(), 1,
                                   static_cast<int>(countInOutput.size()));
                        engine.mixMetronome(countInChannels.data(), 1,
                                            static_cast<int>(countInOutput.size()));
                        countInAligned = countInWindow && countInOffset == 24'000 &&
                                         countInSamples == 128 &&
                                         static_cast<juce::int64>(engine.status().getProperty(
                                             "timelineSample", -1)) == 12'128;
                        countInAudible =
                            *std::max_element(countInOutput.begin(), countInOutput.end()) > 0.0f;
                        engine.stopRecording();
                    }
                    engine.stop();
                    engine.seekToTick(480);
                    if (engine.startRecording(2, error)) {
                        int cancelledOffset = 0;
                        int cancelledSamples = 0;
                        countInCancelled =
                            engine.cancelRecordingIfCountingIn() &&
                            engine.status().getProperty("recordingPhase", {}).toString() ==
                                "idle" &&
                            !engine.recordingWindow(512, cancelledOffset, cancelledSamples) &&
                            cancelledSamples == 0;
                    }
                    engine.play();
                    engine.seekToTick(0);
                    std::array<float, 24000> silent{};
                    std::array<float*, 1> silentChannels{silent.data()};
                    engine.mix(silentChannels.data(), 1, static_cast<int>(silent.size()));
                    looped = static_cast<juce::int64>(
                                 engine.status().getProperty("timelineSample", -1)) == 0;
                    std::array<float, 512> clicks{};
                    std::array<float*, 1> clickChannels{clicks.data()};
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
        addCheck("disabled Instrument placeholder does not block the Graph", loaded);
        addCheck("overlapping sources mix through read-ahead and sample-rate correction", mixed);
        addCheck("tick seek resolves against the engine sample clock", seeked);
        addCheck("loop wrap returns to the exact loop start", looped);
        addCheck("punch range limits the recording window", punchWindowed);
        addCheck("stopped Record without count-in starts Transport", immediateRecordStarted);
        addCheck("count-in and Punch capture share the exact callback offset", countInAligned);
        addCheck("count-in click is generated by the Native Clock", countInAudible);
        addCheck("count-in cancellation returns directly to Idle", countInCancelled);
        addCheck("metronome follows the timeline clock", metronomeMixed);
        addCheck("Volume and Pan Automation ramp within an audio block", automationRamped);
        addCheck("Offline Render writes the exact tick selection", offlineRangeRendered);
        addCheck("Offline Render receives audio from the Arrangement Graph", offlineAudioRendered);
        addCheck("Offline Render normalization reaches the target peak", offlineNormalized);
        addCheck("mix edits swap the Graph without reloading Track Devices",
                 graphUpdateReusedDevices);
        addCheck("Parameter and Bypass changes do not alter Plugin Topology",
                 mutablePluginStateKeepsTopology);
        addCheck("recording taps exclude Timeline playback and Track mix gain",
                 recordingTapIsolated);
        addCheck("Timeline loop capture closes three non-overlapping Audio segments",
                 loopCaptureSegments);
        addCheck("Synthetic Plugin loop recording produces aligned Raw/Processed without stopping",
                 syntheticLoopPassed);
        addCheck("Partial Pass recording generates independent Processed segments",
                 partialPassPassed);
        addCheck("Block Size processing respects prepared block size limit", blockSizePassed);
        addCheck("Long Recording (130 passes) matches Raw/Processed without RAM pre-allocation",
                 longRecordingPassed);
        addCheck(
            "Production ThreadedWriter 4小節×3 Pass (SafetyAudioCallback owns stop, tail flush, "
            "sink clear and session finish)",
            productionWriterPassed);
        addCheck("Production ThreadedWriter Partial Pass", productionWriterPartialPassed);
        addCheck("Stopped Transport processes live Instrument MIDI", liveInstrumentWhileStopped);
        addCheck("Timeline panic closes the Instrument runtime", panicClosesRuntime);
        result->setProperty("checks", checks);
        result->setProperty("message", error);
        result->setProperty("partialSegments", diagPartialSegments);
        result->setProperty("partialRaw", diagPartialRaw);
        result->setProperty("partialProcessed", diagPartialProcessed);
        result->setProperty("partialWindowed", diagPartialWindowed);
        result->setProperty("bsRaw", diagBsRaw);
        result->setProperty("bsProcessed", diagBsProcessed);
        result->setProperty("bsWindowed", diagBsWindowed);
        result->setProperty("partialFailIndex", diagPartialFailIndex);
        result->setProperty("partialFailValue", diagPartialFailValue);
        result->setProperty("partialRawAtFail", diagPartialRawAtFail);
        result->setProperty("automationEarlyLeft", automationEarlyLeft);
        result->setProperty("automationEarlyRight", automationEarlyRight);
        result->setProperty("automationLateLeft", automationLateLeft);
        result->setProperty("automationLateRight", automationLateRight);
        result->setProperty("productionRawSamples",
                            static_cast<juce::int64>(diagProductionRawSamples));
        result->setProperty("productionProcessedSamples",
                            static_cast<juce::int64>(diagProductionProcessedSamples));
        result->setProperty("productionMissingSamples",
                            static_cast<juce::int64>(diagProductionMissing));
        result->setProperty("productionDroppedBlocks",
                            static_cast<juce::int64>(diagProductionDropped));
        result->setProperty("productionPartialRaw",
                            static_cast<juce::int64>(diagProductionPartialRaw));
        result->setProperty("productionPartialProcessed",
                            static_cast<juce::int64>(diagProductionPartialProcessed));
        result->setProperty(
            "passed", sourcesWritten && loaded && mixed && seeked && looped && punchWindowed &&
                          immediateRecordStarted && countInAligned && countInAudible &&
                          countInCancelled && metronomeMixed && automationRamped &&
                          offlineRangeRendered && offlineAudioRendered && offlineNormalized &&
                          graphUpdateReusedDevices && mutablePluginStateKeepsTopology &&
                          recordingTapIsolated && loopCaptureSegments && syntheticLoopPassed &&
                          partialPassPassed && blockSizePassed && longRecordingPassed &&
                          productionWriterPassed && productionWriterPartialPassed &&
                          liveInstrumentWhileStopped && panicClosesRuntime);
        mono.deleteFile();
        stereo.deleteFile();
        directory.getChildFile("offline-selection.wav").deleteFile();
        directory.getChildFile("offline-normalized.wav").deleteFile();
        return juce::var(result);
    }
};

TEST(TimelineEngineTest, CoversTimelinePlaybackRecordingAndRender) {
    test::TemporaryDirectory directory;
    const auto result = TimelineEngineTestPeer::run(directory.get());
    ASSERT_TRUE(result.isObject());

    const auto checks = result.getProperty("checks", {});
    ASSERT_TRUE(checks.isArray());
    for (const auto& check : *checks.getArray()) {
        const auto name = check.getProperty("name", {}).toString();
        EXPECT_TRUE(static_cast<bool>(check.getProperty("passed", false))) << name.toStdString();
    }
    EXPECT_TRUE(static_cast<bool>(result.getProperty("passed", false)));
}

TEST(TimelineEngineTest, LiveMidiTailIncludesEffectChainTail) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(
        engine.loadSnapshot(makeInstrumentSnapshot("track:tail"), formats, 48'000.0, 32, error));
    InstrumentTrace instrumentTrace;
    auto instrument = PluginRackTestPeer::install(
        std::make_unique<TestInstrumentProcessor>(instrumentTrace), 48'000.0, 32, error);
    ASSERT_NE(instrument, nullptr) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:tail",
                                                               std::move(instrument)));
    std::vector<int> effectCalls;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:tail", "effect:tail",
        std::make_unique<TestChainProcessor>(1, 1.0f, 0, effectCalls, 0.25), 48'000.0, 32, error))
        << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::cachePluginTailForTest(engine, "track:tail"));

    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    ASSERT_TRUE(
        engine.enqueueTargetedMidi("track:tail", juce::MidiMessage::noteOn(1, 60, 0.8f), error));
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto callsWithNoteHeld = effectCalls.size();

    // Act
    ASSERT_TRUE(engine.enqueueTargetedMidi("track:tail", juce::MidiMessage::noteOff(1, 60), error));
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto callsAtNoteOff = effectCalls.size();
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

    // Assert
    EXPECT_GT(callsWithNoteHeld, 0u);
    EXPECT_GT(callsAtNoteOff, callsWithNoteHeld);
    EXPECT_GT(effectCalls.size(), callsAtNoteOff);
}

TEST(TimelineEngineTest, FadeShapeEnvelopeMatchesTheRustContract) {
    // Arrange
    // Act / Assert
    // Rust `FadeShape`: 0 linear, 1 equal power, 2 smoothstep.
    EXPECT_NEAR(riffra::fadeEnvelope(0.25f, 0), 0.25f, 1e-6f);
    EXPECT_NEAR(riffra::fadeEnvelope(0.25f, 1), 0.38268343f, 1e-6f);
    EXPECT_NEAR(riffra::fadeEnvelope(0.25f, 2), 0.15625f, 1e-6f);
    for (const int shape : {0, 1, 2}) {
        EXPECT_NEAR(riffra::fadeEnvelope(1.0f, shape), 1.0f, 1e-6f);
        EXPECT_NEAR(riffra::fadeEnvelope(0.0f, shape), 0.0f, 1e-6f);
    }
}

TEST(TimelineEngineTest, KeepsProcessedTakesOutOfTheCurrentTrackEffectChain) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("raw.wav");
    const auto processedFile = directory.get().getChildFile("processed.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 32, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 32, 3'277));

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(makeRawAndProcessedClipSnapshot(rawFile, processedFile),
                                    formats, 48'000.0, 32, error))
        << error.toStdString();
    std::vector<int> processOrder;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:audio", "effect:double",
        std::make_unique<TestChainProcessor>(1, 2.0f, 0, processOrder), 48'000.0, 32, error))
        << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::setPlaybackCompensationForTest(engine, "track:audio", 4));

    constexpr int kBlockSamples = 32;
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputChannels{left.data(), right.data()};

    // Act
    engine.play();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto expected = (0.05f * 2.0f + 0.10f) * 0.5f;

    // Assert
    EXPECT_EQ(processOrder, std::vector<int>{1});
    for (int sample = 0; sample < 4; ++sample) {
        EXPECT_FLOAT_EQ(left[static_cast<std::size_t>(sample)], 0.0f);
        EXPECT_FLOAT_EQ(right[static_cast<std::size_t>(sample)], 0.0f);
    }
    EXPECT_NEAR(left[4], expected, 0.002f);
    EXPECT_NEAR(right[4], expected, 0.002f);
    EXPECT_NEAR(left[31], expected, 0.002f);
    EXPECT_NEAR(right[31], expected, 0.002f);

    // Act: a transport discontinuity must clear both compensation lines.
    engine.stop();
    engine.seekToTick(0);
    std::fill(left.begin(), left.end(), 0.0f);
    std::fill(right.begin(), right.end(), 0.0f);
    engine.play();
    engine.mix(outputChannels.data(), 2, kBlockSamples);

    // Assert
    for (int sample = 0; sample < 4; ++sample) {
        EXPECT_FLOAT_EQ(left[static_cast<std::size_t>(sample)], 0.0f);
        EXPECT_FLOAT_EQ(right[static_cast<std::size_t>(sample)], 0.0f);
    }
    EXPECT_NEAR(left[4], expected, 0.002f);
    EXPECT_NEAR(right[4], expected, 0.002f);
}

TEST(TimelineEngineTest, CompensatesTimelineAudioBeforeMergingMonitoredInput) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("raw-monitor.wav");
    const auto processedFile = directory.get().getChildFile("processed-monitor.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 32, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 32, 3'277));

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile);
    auto* snapshotObject = snapshot.getDynamicObject();
    ASSERT_NE(snapshotObject, nullptr);
    auto tracks = snapshotObject->getProperty("tracks");
    ASSERT_TRUE(tracks.isArray() && tracks.size() == 1);
    auto* track = tracks[0].getDynamicObject();
    ASSERT_NE(track, nullptr);
    track->setProperty("monitoring", "on");
    auto* input = new juce::DynamicObject();
    input->setProperty("channelIndex", 0);
    track->setProperty("audioInput", juce::var(input));

    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(snapshot, formats, 48'000.0, 32, error)) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::setPlaybackCompensationForTest(engine, "track:audio", 4));

    std::array<float, 32> inputSamples{};
    inputSamples.fill(0.25f);
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<const float*, 1> inputs{inputSamples.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    engine.play();
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(left.size()));

    // Assert
    const auto gain = std::sqrt(0.5f);
    EXPECT_NEAR(left[0], 0.25f * gain, 0.002f);
    EXPECT_NEAR(right[0], 0.25f * gain, 0.002f);
    EXPECT_GT(left[4], left[0] + 0.07f);
    EXPECT_NEAR(right[4], left[4], 0.002f);
}

TEST(TimelineEngineTest, ProcessesEachTrackEffectChainOnce) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::trackEffectChainProcessesOnce();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused) {
    EXPECT_TRUE(TimelineEngineTestPeer::canonicalTrackStateSurvivesReusableDeviceCommit());
}

TEST(TimelineEngineTest, KeepsTimelineMidiCompensatedWhileLiveMidiStartsImmediately) {
    EXPECT_TRUE(TimelineEngineTestPeer::timelineMidiKeepsPdcWhenLiveMidiIsImmediate());
}

TEST(TimelineEngineTest, AppliesEditorParameterToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::editorParameterUpdatesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, AppliesPluginStateToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::persistedStateUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, AppliesPluginProgramToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::programChangeUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, SendsEmergencyPanicToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::panicClosesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, RebuildsTimelineForTheCurrentAudioDeviceFormat) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::audioDeviceRestartRebuildsRuntimeFormat();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, RendersBuiltInInstrumentThroughTimelineLiveAndLoopPaths) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(makeBuiltInInstrumentSnapshot("track:builtin", true, true),
                                    formats, 48'000.0, 512, error))
        << error.toStdString();

    std::vector<int> processOrder;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:builtin", "effect:builtin",
        std::make_unique<TestChainProcessor>(7, 2.0f, 0, processOrder), 48'000.0, 512, error))
        << error.toStdString();

    constexpr int kBlockSamples = 512;
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputChannels{left.data(), right.data()};
    const auto outputMagnitude = [&] {
        return std::max(std::max(std::abs(*std::max_element(left.begin(), left.end())),
                                 std::abs(*std::min_element(left.begin(), left.end()))),
                        std::max(std::abs(*std::max_element(right.begin(), right.end())),
                                 std::abs(*std::min_element(right.begin(), right.end()))));
    };
    const auto clearOutput = [&] {
        std::fill(left.begin(), left.end(), 0.0f);
        std::fill(right.begin(), right.end(), 0.0f);
    };

    // Act: timeline MIDI is rendered through the built-in runtime and its
    // effect chain, then a loop boundary resets and schedules it again.
    engine.play();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto timelinePeak = outputMagnitude();
    auto loopPeak = 0.0f;
    for (int block = 0; block < 16; ++block) {
        clearOutput();
        engine.mix(outputChannels.data(), 2, kBlockSamples);
        loopPeak = std::max(loopPeak, outputMagnitude());
    }

    // A seek must reset the built-in runtime before the next timeline note.
    engine.seekToTick(0);
    engine.play();
    clearOutput();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto seekPeak = outputMagnitude();

    // Stopped transport still accepts targeted live MIDI, including directly
    // after stop/reset.
    engine.stop();
    ASSERT_TRUE(
        engine.enqueueTargetedMidi("track:builtin", juce::MidiMessage::noteOn(1, 64, 0.8f), error))
        << error.toStdString();
    clearOutput();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto livePeak = outputMagnitude();

    engine.stop();
    ASSERT_TRUE(
        engine.enqueueTargetedMidi("track:builtin", juce::MidiMessage::noteOn(1, 67, 0.7f), error))
        << error.toStdString();
    clearOutput();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto liveAfterStopPeak = outputMagnitude();

    // Assert
    EXPECT_GT(timelinePeak, 0.0f);
    EXPECT_GT(loopPeak, 0.0f);
    EXPECT_GT(seekPeak, 0.0f);
    EXPECT_GT(livePeak, 0.0f);
    EXPECT_GT(liveAfterStopPeak, 0.0f);
    ASSERT_FALSE(processOrder.empty());
    EXPECT_EQ(processOrder.front(), 7);
    const auto armedTrackIds = engine.status().getProperty("armedTrackIds", {});
    ASSERT_TRUE(armedTrackIds.isArray());
    EXPECT_EQ(armedTrackIds.size(), 1);
}

TEST(TimelineEngineTest, RendersBuiltInInstrumentThroughOfflineRenderer) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    const auto destination = directory.get().getChildFile("built-in.wav");
    OfflineRenderer renderer;
    OfflineRenderer::Result result;
    juce::String error;

    // Act
    const auto rendered =
        renderer.render(makeBuiltInInstrumentSnapshot("track:offline"), formats, destination, 0,
                        960, 48'000.0, 512, 0.0f, false, result, error);

    // Assert
    ASSERT_TRUE(rendered) << error.toStdString();
    ASSERT_TRUE(destination.existsAsFile());
    ASSERT_GT(result.frames, 0u);
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
    ASSERT_NE(reader, nullptr);
    ASSERT_EQ(reader->numChannels, 2u);
    ASSERT_GT(reader->lengthInSamples, 0);
    const auto frameCount = static_cast<int>(reader->lengthInSamples);
    juce::AudioBuffer<float> output(2, frameCount);
    ASSERT_TRUE(reader->read(&output, 0, frameCount, 0, true, true));
    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        for (int sample = 0; sample < output.getNumSamples(); ++sample)
            ASSERT_TRUE(std::isfinite(output.getSample(channel, sample)));
    EXPECT_GT(
        std::max(output.getMagnitude(0, 0, frameCount), output.getMagnitude(1, 0, frameCount)),
        0.0f);
}

TEST(TimelineEngineTest, MonitorsAudioTrackInputWhileTransportIsStopped) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;

    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    const auto makeTrack = [&timebase](const juce::String& id, const bool muted) {
        auto* track = new juce::DynamicObject();
        track->setProperty("id", id);
        track->setProperty("kind", "audio");
        track->setProperty("gainDb", 0.0);
        track->setProperty("pan", 0.0);
        track->setProperty("muted", muted);
        track->setProperty("solo", false);
        track->setProperty("armed", false);
        track->setProperty("monitoring", "on");
        auto* audioInput = new juce::DynamicObject();
        audioInput->setProperty("channelIndex", 0);
        track->setProperty("audioInput", juce::var(audioInput));
        auto* rack = new juce::DynamicObject();
        rack->setProperty("devices", juce::Array<juce::var>{});
        track->setProperty("rack", juce::var(rack));
        track->setProperty("audioClips", juce::Array<juce::var>{});
        track->setProperty("midiClips", juce::Array<juce::var>{});
        track->setProperty("automation", juce::Array<juce::var>{});
        return juce::var(track);
    };

    juce::Array<juce::var> tracks;
    tracks.add(makeTrack("track:guitar", false));
    tracks.add(makeTrack("track:muted-guitar", true));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);

    ASSERT_TRUE(engine.loadSnapshot(juce::var(snapshot), formats, 48'000.0, 512, error));

    constexpr int kBlockSamples = 512;
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputChannels{input.data()};
    const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};
    const auto outputMagnitude = [&] {
        return std::max(
            juce::FloatVectorOperations::findMaximum(outputLeft.data(), kBlockSamples),
            juce::FloatVectorOperations::findMaximum(outputRight.data(), kBlockSamples));
    };

    // Act: monitor the physical input without starting the transport.
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);

    // Assert: the stopped transport still routes the Audio Track input to the
    // output, while a muted track stays silent. Pan law halves the level.
    EXPECT_GT(outputMagnitude(), 0.02f);
    EXPECT_LT(outputMagnitude(), 0.08f);

    engine.play();
    std::fill(outputLeft.begin(), outputLeft.end(), 0.0f);
    std::fill(outputRight.begin(), outputRight.end(), 0.0f);
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
    EXPECT_GT(outputMagnitude(), 0.02f);
    EXPECT_LT(outputMagnitude(), 0.08f);
}

TEST(TimelineEngineTest, MonitorsAudioTrackInputOncePerAudioCallback) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    constexpr int kBlockSamples = 512;
    const auto measurePeak = [&](const int trackCount, float& peak) {
        TimelineEngine engine;
        juce::String error;
        if (!engine.loadSnapshot(makeAudioTrackSnapshot(trackCount, true, false), formats, 48'000.0,
                                 kBlockSamples, error))
            return false;
        std::array<float, kBlockSamples> input{};
        input.fill(0.05f);
        std::array<float, kBlockSamples> outputLeft{};
        std::array<float, kBlockSamples> outputRight{};
        const std::array<const float*, 1> inputChannels{input.data()};
        const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};

        engine.play();
        engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
        peak =
            std::max(juce::FloatVectorOperations::findMaximum(outputLeft.data(), kBlockSamples),
                     juce::FloatVectorOperations::findMaximum(outputRight.data(), kBlockSamples));
        return true;
    };
    float oneTrackPeak = 0.0f;
    float twoTrackPeak = 0.0f;
    float tenTrackPeak = 0.0f;

    // Act
    ASSERT_TRUE(measurePeak(1, oneTrackPeak));
    ASSERT_TRUE(measurePeak(2, twoTrackPeak));
    ASSERT_TRUE(measurePeak(10, tenTrackPeak));

    // Assert
    EXPECT_GT(oneTrackPeak, 0.02f);
    EXPECT_NEAR(twoTrackPeak, oneTrackPeak, 0.0001f);
    EXPECT_NEAR(tenTrackPeak, oneTrackPeak, 0.0001f);
}

TEST(TimelineEngineTest, KeepsAudioCaptureOpenForTheWholeAudioCallback) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(engine.loadSnapshot(makeAudioTrackSnapshot(10, false, true), formats, 48'000.0,
                                    kBlockSamples, error));
    CaptureIsolationSink captureSink;
    engine.setRecordingSink(&captureSink);
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputChannels{input.data()};
    const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(engine.startRecording(0, error));
    ASSERT_TRUE(engine.recordingWindow(kBlockSamples, captureOffset, captureSamples));

    // Act
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
    const auto rawSamplesAfterCallback = captureSink.totalRawSamples;
    const auto beginCountAfterCallback = captureSink.beginCount;
    const auto endCountAfterCallback = captureSink.endCount;
    engine.stopRecording();
    const auto flushed = engine.flushRecordingTail(error);
    engine.stop();
    engine.clearRecordingSink();

    // Assert
    EXPECT_TRUE(flushed);
    EXPECT_EQ(captureOffset, 0);
    EXPECT_EQ(captureSamples, kBlockSamples);
    EXPECT_EQ(rawSamplesAfterCallback, kBlockSamples);
    EXPECT_EQ(beginCountAfterCallback, 1);
    EXPECT_EQ(endCountAfterCallback, 0);
    EXPECT_EQ(captureSink.totalRawSamples, kBlockSamples);
    EXPECT_EQ(captureSink.beginCount, 1);
    EXPECT_EQ(captureSink.endCount, 1);
}

TEST(TimelineEngineTest, MonitorsAudioTrackInputThroughTheTrackEffectChain) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;

    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* track = new juce::DynamicObject();
    track->setProperty("id", "track:guitar");
    track->setProperty("kind", "audio");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", false);
    track->setProperty("monitoring", "on");
    auto* audioInput = new juce::DynamicObject();
    audioInput->setProperty("channelIndex", 0);
    track->setProperty("audioInput", juce::var(audioInput));
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", juce::Array<juce::var>{});
    track->setProperty("midiClips", juce::Array<juce::var>{});
    track->setProperty("automation", juce::Array<juce::var>{});

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);

    ASSERT_TRUE(engine.loadSnapshot(juce::var(snapshot), formats, 48'000.0, 512, error));
    std::vector<int> processOrder;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:guitar", "device:amp",
        std::make_unique<TestChainProcessor>(1, 2.0f, 0, processOrder), 48'000.0, 512, error));

    constexpr int kBlockSamples = 512;
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputChannels{input.data()};
    const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};

    // Act: monitor the physical input through the amplifier device first
    // without the transport, then with the transport playing.
    std::fill(outputLeft.begin(), outputLeft.end(), 0.0f);
    std::fill(outputRight.begin(), outputRight.end(), 0.0f);
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
    const auto stoppedPeak =
        std::max(juce::FloatVectorOperations::findMaximum(outputLeft.data(), kBlockSamples),
                 juce::FloatVectorOperations::findMaximum(outputRight.data(), kBlockSamples));
    std::fill(outputLeft.begin(), outputLeft.end(), 0.0f);
    std::fill(outputRight.begin(), outputRight.end(), 0.0f);
    engine.seekToTick(0);
    engine.play();
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);

    // Assert: the live chain processed the input (2x gain, pan law) and the
    // monitored signal reaches the output while the transport is stopped and
    // while it is playing, instead of being turned into silence.
    const auto playingPeak =
        std::max(juce::FloatVectorOperations::findMaximum(outputLeft.data(), kBlockSamples),
                 juce::FloatVectorOperations::findMaximum(outputRight.data(), kBlockSamples));
    EXPECT_GT(stoppedPeak, 0.05f);
    EXPECT_LT(stoppedPeak, 0.2f);
    EXPECT_GT(playingPeak, 0.05f);
    EXPECT_LT(playingPeak, 0.2f);
    EXPECT_EQ(processOrder.size(), 2u);
}

}  // namespace riffra
