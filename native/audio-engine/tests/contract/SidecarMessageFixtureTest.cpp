#include <gtest/gtest.h>

#include <cstdlib>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "JsonTestSupport.h"
#include "contract/SidecarMessages.h"

namespace riffra {
namespace {

using namespace json_test;

juce::File messageFixtureDirectory() {
    return juce::File(RIFFRA_SIDECAR_FIXTURE_DIR).getChildFile("messages");
}

PluginStateSpec pluginState() { return {juce::String("opaque-state"), {0.25f, 0.75f}, false}; }

AudioStatusSpec audioStatus() {
    AudioStatusSpec status;
    status.state = AudioStateSpec::muted;
    status.message = "Native audio is connected and muted.";
    status.driver = juce::String("ASIO");
    status.inputDevice = juce::String("Interface");
    status.inputChannel = 0u;
    status.inputChannels = {{0, "Input 1"}, {1, "Input 2"}};
    status.activeInputChannels = {0, 1};
    status.outputDevice = std::nullopt;
    status.outputChannels = {{0, "Output 1"}};
    status.activeOutputChannels = {0};
    status.sampleRate = 48'000.0;
    status.bufferSize = 256u;
    status.roundTripMs = 10.5;
    status.timelineTick = 960u;
    status.recording.active = true;
    status.recording.directory = juce::String("recordings/take-1");
    status.recording.sampleRate = 48'000.0;
    status.recording.samplesWritten = 4'800;
    status.recording.droppedBlocks = 1;
    status.recording.rawMissingSamples = 32;
    status.recording.rawDropoutStartSample = 100u;
    status.recording.rawDropoutEndSample = 132u;
    status.recording.recoveryStatus = RecoveryStatusSpec::partial;
    status.recording.error = std::nullopt;
    status.midiInputs = {{"midi-in-1", "Keyboard"}};
    status.midiOutputs = {};
    status.midiInputActive = true;
    status.midiMessages = 12;
    status.lastMidiNote = static_cast<std::uint8_t>(60);
    status.inputPeak = 0.25;
    status.outputPeak = 0.5;
    status.invalidSamples = 3;
    status.feedbackSuspected = false;
    status.previewing = true;
    status.instrumentPreviewing = false;
    status.muteReasons = 1;
    status.diagnostics.callbackCount = 1'000;
    status.diagnostics.averageCallbackDurationUs = 120;
    status.diagnostics.maximumCallbackDurationUs = 800;
    status.diagnostics.callbackOverruns = 2;
    status.diagnostics.preLimiterPeak = 0.75;
    status.diagnostics.limiterGainReductionDb = 1.5;
    status.diagnostics.hardClipSamples = 4;
    status.diagnostics.liveMidiDrops = 1;
    status.diagnostics.graphRevision = 7;
    status.diagnostics.graphPublishCount = 3;
    status.diagnostics.trackCount = 2;
    status.diagnostics.instrumentRuntimeCount = 1;
    status.diagnostics.pluginCount = 1;
    status.diagnostics.maximumLatencySamples = 64;
    status.diagnostics.instrumentFaults = {{"track-1", "vst3", 0, 1}};
    return status;
}

TransportStatusSpec transportStatus() {
    TransportStatusSpec status;
    status.state = TransportStateSpec::playing;
    status.revision = 7u;
    status.timelineTick = 1'920;
    status.sequence = 42;
    status.recordingPhase = RecordingPhaseSpec::recording;
    status.recordingStartTick = 960;
    status.recordingPassOrdinal = 1;
    status.armedTrackIds = {"track-1"};
    status.clockGeneration = 2;
    status.discontinuity = 5;
    return status;
}

AudioMetersSpec audioMeters() {
    AudioMetersSpec meters;
    meters.projectId = juce::String("project-1");
    meters.inputPeak = 0.25;
    meters.outputPeak = 0.5;
    meters.outputPeakLeft = 0.5;
    meters.outputPeakRight = 0.25;
    meters.invalidSamples = 0;
    meters.preLimiterPeak = 0.5;
    meters.limiterGainReductionDb = 0.0;
    meters.hardClipSamples = 0;
    meters.muteReasons = 0;
    meters.trackMeters = {{"track-1", 0.5, 0.25, 0.125, 0.0625}};
    return meters;
}

SidecarErrorSpec error() {
    return {"deviceLost", "The audio device disappeared.", "audioDevice.recover", {}};
}

std::vector<SidecarResponseSpec> responses() {
    TrackDeviceParametersSpec parameters;
    parameters.parameters = {{0, "Gain", 0.5f, 0.25f, true}};
    TrackDeviceProgramsSpec programs;
    programs.currentIndex = 1u;
    programs.programs = {{0, "Init"}, {1, "Bright"}};
    return {
        audioStatus(),
        transportStatus(),
        AckSpec{},
        TrackDeviceStatusSpec{"Test Effect", false, 1, {true, true, true, false}},
        parameters,
        programs,
        TrackPluginStateSpec{pluginState()},
    };
}

std::vector<SidecarEventSpec> events() {
    return {
        ReadySpec{audioStatus()},
        audioStatus(),
        audioMeters(),
        transportStatus(),
        RecordingCompleteSpec{"recordings/take-1", false, juce::String("processing failed")},
        TrackPluginStateChangedSpec{"project-1", "track-1", "device-1", pluginState()},
        TrackPluginParameterChangedSpec{"project-1", "track-1", "device-1", 2, 0.5f},
        FaultSpec{error()},
    };
}

std::vector<std::pair<juce::String, juce::var>> messageFixtures() {
    std::vector<std::pair<juce::String, juce::var>> fixtures;
    for (const auto& response : responses())
        fixtures.emplace_back("response." +
                                  juce::String(std::string(sidecarResponseType(response)).c_str()) +
                                  ".json",
                              encodeResponse(1, response));
    for (const auto& event : events())
        fixtures.emplace_back(
            "event." + juce::String(std::string(sidecarEventType(event)).c_str()) + ".json",
            encodeEvent(event));
    fixtures.emplace_back("error.json", encodeError(1, error()));
    fixtures.emplace_back("render.offlineRenderComplete.json",
                          encodeOfflineRenderComplete({96'000, 48'000}));
    fixtures.emplace_back(
        "render.error.json",
        encodeOfflineRenderError(
            {"renderRejected", "Offline Render request is invalid.", "renderTimelineOffline", {}}));
    return fixtures;
}

bool updateRequested() {
    const auto* update = std::getenv("RIFFRA_UPDATE_CONTRACT_FIXTURES");
    return update != nullptr && std::string(update) == "1";
}

TEST(SidecarMessageFixtureTest, EveryMessageTypeHasASample) {
    std::set<std::string_view> responseTypes;
    for (const auto& response : responses()) responseTypes.insert(sidecarResponseType(response));
    std::set<std::string_view> eventTypes;
    for (const auto& event : events()) eventTypes.insert(sidecarEventType(event));

    EXPECT_EQ(responseTypes, std::set<std::string_view>(kSidecarResponseTypes.begin(),
                                                        kSidecarResponseTypes.end()));
    EXPECT_EQ(eventTypes,
              std::set<std::string_view>(kSidecarEventTypes.begin(), kSidecarEventTypes.end()));
}

TEST(SidecarMessageFixtureTest, MessageFixturesAreCurrent) {
    const auto directory = messageFixtureDirectory();
    const auto fixtures = messageFixtures();
    if (updateRequested()) {
        ASSERT_TRUE(directory.deleteRecursively());
        ASSERT_TRUE(directory.createDirectory());
        for (const auto& [name, value] : fixtures)
            ASSERT_TRUE(directory.getChildFile(name).replaceWithText(
                juce::JSON::toString(value) + "\n", false, false, "\n"));
    }

    std::set<juce::String> expected;
    for (const auto& [name, value] : fixtures) {
        expected.insert(name);
        const auto file = directory.getChildFile(name);
        ASSERT_TRUE(file.existsAsFile()) << "missing message fixture: " << name;
        EXPECT_TRUE(jsonEquals(readJsonFile(file), value)) << "stale message fixture: " << name;
    }
    std::set<juce::String> actual;
    for (const auto& file : directory.findChildFiles(juce::File::findFiles, false, "*.json"))
        actual.insert(file.getFileName());
    EXPECT_EQ(actual, expected);
}

}  // namespace
}  // namespace riffra
