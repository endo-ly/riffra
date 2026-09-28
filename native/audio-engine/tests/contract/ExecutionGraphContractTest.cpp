#include <gtest/gtest.h>

#include <string>
#include <utility>
#include <vector>

#include "JsonTestSupport.h"
#include "contract/ExecutionGraphDecoder.h"

namespace riffra {
namespace {

using namespace json_test;

juce::var fixture(const char* name) {
    return readJsonFile(juce::File(RIFFRA_CONTRACT_FIXTURE_DIR).getChildFile(name));
}

void expectTimelineSnapshotRejected(const juce::var& value, const juce::String& expectedPath = {}) {
    TimelineSnapshotSpec decoded;
    juce::String error;
    EXPECT_FALSE(decodeTimelineSnapshot(value, decoded, error));
    EXPECT_TRUE(error.isNotEmpty());
    if (expectedPath.isNotEmpty()) EXPECT_TRUE(error.contains(expectedPath)) << error.toStdString();
}

void expectTimelineSnapshotAccepted(const juce::var& value) {
    TimelineSnapshotSpec decoded;
    juce::String error;
    ASSERT_TRUE(decodeTimelineSnapshot(value, decoded, error)) << error.toStdString();
}

TEST(ExecutionGraphContractTest, DecodesRustGeneratedFixtures) {
    TimelineSnapshotSpec minimal;
    juce::String error;
    ASSERT_TRUE(decodeTimelineSnapshot(fixture("timeline-snapshot-minimal.json"), minimal, error))
        << error.toStdString();
    EXPECT_EQ(minimal.projectId, "project-minimal");

    TimelineSnapshotSpec full;
    error.clear();
    ASSERT_TRUE(decodeTimelineSnapshot(fixture("timeline-snapshot-full.json"), full, error))
        << error.toStdString();
    EXPECT_EQ(full.graph.tracks.size(), 3u);
    EXPECT_EQ(full.graph.tracks[1].instrument->index(), 1u);
    EXPECT_EQ(full.graph.tracks[0].kind, TrackKindSpec::audio);
    EXPECT_EQ(full.graph.tracks[1].kind, TrackKindSpec::instrument);
    EXPECT_EQ(full.graph.tracks[0].monitoring, MonitoringSpec::off);
    EXPECT_EQ(full.graph.tracks[1].monitoring, MonitoringSpec::automatic);
    EXPECT_EQ(full.graph.tracks[2].monitoring, MonitoringSpec::on);
    EXPECT_EQ(full.graph.tracks[0].audioClips[0].fadeShape, FadeShapeSpec::linear);
    EXPECT_EQ(full.graph.tracks[0].audioClips[1].fadeShape, FadeShapeSpec::equalPower);
    EXPECT_EQ(full.graph.tracks[0].audioClips[2].fadeShape, FadeShapeSpec::smooth);
    EXPECT_EQ(full.graph.tracks[0].audioClips[0].takeVariant, TakeVariantSpec::raw);
    EXPECT_EQ(full.graph.tracks[0].audioClips[1].takeVariant, TakeVariantSpec::processed);
    EXPECT_EQ(full.graph.tracks[1].midiClips[0].events[0].kind, MidiEventKindSpec::pitchBend);
    EXPECT_EQ(full.graph.tracks[2].midiClips[0].events[0].kind, MidiEventKindSpec::channelPressure);
    EXPECT_EQ(full.graph.tracks[2].midiClips[0].events[1].kind, MidiEventKindSpec::controlChange);
    EXPECT_FALSE(full.graph.tracks[0].instrument.has_value());
    EXPECT_FALSE(full.graph.tracks[1].audioInput.has_value());
    EXPECT_FALSE(full.graph.tracks[0].midiInput.deviceId.has_value());
    EXPECT_FALSE(full.graph.tracks[0].midiInput.channel.has_value());
    EXPECT_TRUE(full.graph.tracks[0].audioInput.has_value());
    EXPECT_TRUE(full.graph.punchRange.has_value());
    EXPECT_TRUE(full.graph.tracks[2].instrument.has_value());
    EXPECT_FALSE(
        std::get<Vst3InstrumentSpec>(*full.graph.tracks[2].instrument).state.stateData.has_value());

    OfflineRenderRequestSpec render;
    error.clear();
    ASSERT_TRUE(decodeOfflineRenderRequest(fixture("offline-render-request.json"), render, error))
        << error.toStdString();
    EXPECT_EQ(render.graph.masterGainDb, -6.0);
    EXPECT_EQ(render.destination, "renders/song.wav");

    PluginStateSpec state;
    error.clear();
    ASSERT_TRUE(decodePluginState(fixture("plugin-state.json"), state, error))
        << error.toStdString();
    EXPECT_EQ(state.parameterValues.size(), 2u);
}

TEST(ExecutionGraphContractTest, DecodesEveryFixtureInTheContractDirectory) {
    const juce::File directory(RIFFRA_CONTRACT_FIXTURE_DIR);
    const auto files = directory.findChildFiles(juce::File::findFiles, false, "*.json");
    ASSERT_FALSE(files.isEmpty());
    for (const auto& file : files) {
        const auto value = juce::JSON::parse(file.loadFileAsString());
        juce::String error;
        if (file.getFileName() == "timeline-snapshot-minimal.json" ||
            file.getFileName() == "timeline-snapshot-full.json") {
            TimelineSnapshotSpec decoded;
            ASSERT_TRUE(decodeTimelineSnapshot(value, decoded, error)) << error.toStdString();
        } else if (file.getFileName() == "offline-render-request.json") {
            OfflineRenderRequestSpec decoded;
            ASSERT_TRUE(decodeOfflineRenderRequest(value, decoded, error)) << error.toStdString();
        } else if (file.getFileName() == "plugin-state.json") {
            PluginStateSpec decoded;
            ASSERT_TRUE(decodePluginState(value, decoded, error)) << error.toStdString();
        } else {
            FAIL() << "unhandled execution graph fixture: " << file.getFileName().toStdString();
        }
    }
}

TEST(ExecutionGraphContractTest, RejectsMissingAndUnknownKeysAtEveryObjectDepth) {
    const auto full = fixture("timeline-snapshot-full.json");
    std::vector<JsonPath> paths;
    JsonPath path;
    collectObjectPaths(full, path, paths);

    std::vector<std::pair<JsonPath, std::string>> fieldPaths;
    collectFieldPaths(full, path, fieldPaths);
    for (const auto& [objectPath, key] : fieldPaths) {
        const auto missing = withoutKey(full, objectPath, key);
        auto expectedPath = objectPath;
        expectedPath.push_back(key);
        expectTimelineSnapshotRejected(missing, formatPath(expectedPath));
    }

    for (const auto& objectPath : paths) {
        const auto unknown = withUnknownKey(full, objectPath);
        const auto expectedPath = formatPath(objectPath);
        expectTimelineSnapshotRejected(unknown, expectedPath.isEmpty()
                                                    ? juce::String("__unexpected")
                                                    : expectedPath + ".__unexpected");
    }
}

TEST(ExecutionGraphContractTest, RejectsValuesOutsideContractRanges) {
    const auto full = fixture("timeline-snapshot-full.json");
    const std::vector<std::pair<JsonPath, juce::var>> invalidValues{
        {JsonPath{"graph", "timebase", "ppq"}, 1},
        {JsonPath{"graph", "timebase", "bpm"}, 19.0},
        {JsonPath{"graph", "timebase", "bpm"}, 401.0},
        {JsonPath{"graph", "timebase", "timeSignatureNumerator"}, 0},
        {JsonPath{"graph", "timebase", "timeSignatureDenominator"}, 0},
        {JsonPath{"graph", "masterGainDb"}, -91.0},
        {JsonPath{"graph", "masterGainDb"}, 1.0},
        {JsonPath{"graph", "tracks", "#0", "gainDb"}, -91.0},
        {JsonPath{"graph", "tracks", "#0", "gainDb"}, 25.0},
        {JsonPath{"graph", "tracks", "#0", "pan"}, -1.1},
        {JsonPath{"graph", "tracks", "#0", "pan"}, 1.1},
        {JsonPath{"graph", "tracks", "#0", "audioInput", "channelIndex"}, 32},
        {JsonPath{"graph", "tracks", "#0", "volumeAutomation", "#0", "value"}, -91.0},
        {JsonPath{"graph", "tracks", "#0", "volumeAutomation", "#0", "value"}, 25.0},
        {JsonPath{"graph", "tracks", "#0", "panAutomation", "#0", "value"}, -1.1},
        {JsonPath{"graph", "tracks", "#0", "panAutomation", "#0", "value"}, 1.1},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "pan"}, 1.1},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "gainDb"}, -91.0},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "gainDb"}, 25.0},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "sourceSampleRate"}, 0},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "sourceEndFrame"}, 0},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "durationFrames"}, 0},
        {JsonPath{"graph", "tracks", "#0", "audioClips", "#0", "durationSampleRate"}, 0},
        {JsonPath{"graph", "tracks", "#1", "midiInput", "channel"}, 17},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "channel"}, 0},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "note"}, 128},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "velocity"}, 0},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "channel"}, 17},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data1"}, 128},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data2"}, 128},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "channel"}, 0},
        {JsonPath{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "channel"}, 17},
    };
    for (const auto& [pathToValue, invalid] : invalidValues) {
        auto mutated = cloneJson(full);
        setValue(mutated, pathToValue, invalid);
        expectTimelineSnapshotRejected(mutated);
    }

    auto invalidLoop = cloneJson(full);
    setValue(invalidLoop, {"graph", "loopRange", "endTick"}, 120);
    expectTimelineSnapshotRejected(invalidLoop);
    auto invalidPunch = cloneJson(full);
    setValue(invalidPunch, {"graph", "punchRange", "endTick"}, 240);
    expectTimelineSnapshotRejected(invalidPunch);
    auto invalidMidiDuration = cloneJson(full);
    setValue(invalidMidiDuration, {"graph", "tracks", "#1", "midiClips", "#0", "durationTicks"}, 0);
    expectTimelineSnapshotRejected(invalidMidiDuration);
    auto invalidMidiPosition = cloneJson(full);
    setValue(invalidMidiPosition,
             {"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "tick"}, 960);
    expectTimelineSnapshotRejected(invalidMidiPosition);

    const std::vector<std::pair<JsonPath, juce::var>> validBoundaries{
        {{"graph", "timebase", "bpm"}, 20.0},
        {{"graph", "timebase", "bpm"}, 400.0},
        {{"graph", "timebase", "timeSignatureNumerator"}, 1},
        {{"graph", "timebase", "timeSignatureNumerator"}, 255},
        {{"graph", "timebase", "timeSignatureDenominator"}, 1},
        {{"graph", "timebase", "timeSignatureDenominator"}, 255},
        {{"graph", "masterGainDb"}, -90.0},
        {{"graph", "masterGainDb"}, 0.0},
        {{"graph", "tracks", "#0", "gainDb"}, -90.0},
        {{"graph", "tracks", "#0", "gainDb"}, 24.0},
        {{"graph", "tracks", "#0", "pan"}, -1.0},
        {{"graph", "tracks", "#0", "pan"}, 1.0},
        {{"graph", "tracks", "#0", "volumeAutomation", "#0", "value"}, -90.0},
        {{"graph", "tracks", "#0", "volumeAutomation", "#0", "value"}, 24.0},
        {{"graph", "tracks", "#0", "panAutomation", "#0", "value"}, -1.0},
        {{"graph", "tracks", "#0", "panAutomation", "#0", "value"}, 1.0},
        {{"graph", "tracks", "#0", "audioClips", "#0", "pan"}, -1.0},
        {{"graph", "tracks", "#0", "audioClips", "#0", "pan"}, 1.0},
        {{"graph", "tracks", "#0", "audioClips", "#0", "gainDb"}, -90.0},
        {{"graph", "tracks", "#0", "audioClips", "#0", "gainDb"}, 24.0},
        {{"graph", "tracks", "#0", "audioInput", "channelIndex"}, 0},
        {{"graph", "tracks", "#0", "audioInput", "channelIndex"}, 31},
        {{"graph", "tracks", "#0", "audioClips", "#0", "sourceSampleRate"}, 1},
        {{"graph", "tracks", "#0", "audioClips", "#0", "sourceEndFrame"}, 1},
        {{"graph", "tracks", "#0", "audioClips", "#0", "durationFrames"}, 1},
        {{"graph", "tracks", "#0", "audioClips", "#0", "durationSampleRate"}, 1},
        {{"graph", "tracks", "#1", "midiInput", "channel"}, 1},
        {{"graph", "tracks", "#1", "midiInput", "channel"}, 16},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "startTick"}, 0},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "durationTicks"}, 1},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "note"}, 0},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "note"}, 127},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "velocity"}, 1},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "velocity"}, 127},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "channel"}, 1},
        {{"graph", "tracks", "#1", "midiClips", "#0", "notes", "#0", "channel"}, 16},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "tick"}, 0},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "channel"}, 1},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "channel"}, 16},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data1"}, 0},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data1"}, 127},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data2"}, 0},
        {{"graph", "tracks", "#1", "midiClips", "#0", "events", "#0", "data2"}, 127},
    };
    for (const auto& [pathToValue, boundary] : validBoundaries) {
        auto mutated = cloneJson(full);
        setValue(mutated, pathToValue, boundary);
        expectTimelineSnapshotAccepted(mutated);
    }

    auto oneTickMidiClip = cloneJson(full);
    setValue(oneTickMidiClip, {"graph", "tracks", "#2", "midiClips", "#0", "durationTicks"}, 1);
    setValue(oneTickMidiClip, {"graph", "tracks", "#2", "midiClips", "#0", "events", "#0", "tick"},
             0);
    setValue(oneTickMidiClip, {"graph", "tracks", "#2", "midiClips", "#0", "events", "#1", "tick"},
             0);
    expectTimelineSnapshotAccepted(oneTickMidiClip);

    auto minimumValidRanges = cloneJson(full);
    setValue(minimumValidRanges, {"graph", "loopRange", "startTick"}, 0);
    setValue(minimumValidRanges, {"graph", "loopRange", "endTick"}, 1);
    setValue(minimumValidRanges, {"graph", "punchRange", "startTick"}, 0);
    setValue(minimumValidRanges, {"graph", "punchRange", "endTick"}, 1);
    expectTimelineSnapshotAccepted(minimumValidRanges);
}

}  // namespace
}  // namespace riffra
