#include <gtest/gtest.h>

#include <array>

#include "ArrangeRecordingSession.h"
#include "TestSupport.h"

namespace riffra {
namespace {

juce::var makeConfiguration() {
    auto* configuration = new juce::DynamicObject();
    configuration->setProperty("sampleRate", 48'000.0);
    configuration->setProperty("timelineStartTick", 960);
    configuration->setProperty("loopEnabled", true);
    configuration->setProperty("loopStartSample", 24'000);
    configuration->setProperty("loopEndSample", 48'000);
    configuration->setProperty("punchEnabled", false);

    juce::Array<juce::var> tracks;
    const auto addTrack = [&tracks](const juce::String& id, const juce::String& kind, int input) {
        auto* track = new juce::DynamicObject();
        track->setProperty("trackId", id);
        track->setProperty("kind", kind);
        track->setProperty("audioInputChannel", input);
        track->setProperty("pluginLatencySamples", input + 8);
        track->setProperty("pluginTailSamples", input + 16);
        tracks.add(juce::var(track));
    };
    addTrack("track:guitar", "audio", 0);
    addTrack("track:vocal", "audio", 1);
    addTrack("track:keys", "instrument", -1);
    configuration->setProperty("tracks", tracks);
    return juce::var(configuration);
}

bool writeCapture(ArrangeRecordingSession& session, juce::String& error) {
    std::array<float, 512> guitarRaw{};
    std::array<float, 512> guitarLeft{};
    std::array<float, 512> guitarRight{};
    std::array<float, 512> vocalRaw{};
    std::array<float, 512> vocalLeft{};
    std::array<float, 512> vocalRight{};
    guitarRaw.fill(0.1f);
    guitarLeft.fill(0.2f);
    guitarRight.fill(0.21f);
    vocalRaw.fill(0.3f);
    vocalLeft.fill(0.4f);
    vocalRight.fill(0.41f);
    const std::array<const float*, 2> guitarProcessed{guitarLeft.data(), guitarRight.data()};
    const std::array<const float*, 2> vocalProcessed{vocalLeft.data(), vocalRight.data()};

    session.setCaptureRange(1000, 1256, 24'000, 24'256);
    session.setCaptureRange(1256, 1512, 24'000, 24'256);
    if (!session.beginAudioTrackCapture("track:guitar", 1000, 24'000)) return false;
    session.writeAudioTrack("track:guitar", guitarRaw.data(), 256, guitarProcessed.data(), 256);
    session.endAudioTrackCapture("track:guitar", 1256, 24'256);
    session.completeAudioTrackTail("track:guitar");
    if (!session.beginAudioTrackCapture("track:guitar", 1256, 24'000)) return false;
    const std::array<const float*, 2> guitarProcessedSecond{guitarLeft.data() + 256,
                                                            guitarRight.data() + 256};
    session.writeAudioTrack("track:guitar", guitarRaw.data() + 256, 256,
                            guitarProcessedSecond.data(), 256);
    session.endAudioTrackCapture("track:guitar", 1512, 24'256);
    session.completeAudioTrackTail("track:guitar");

    if (!session.beginAudioTrackCapture("track:vocal", 1000, 24'000)) return false;
    session.writeAudioTrack("track:vocal", vocalRaw.data(), 256, vocalProcessed.data(), 256);
    session.endAudioTrackCapture("track:vocal", 1256, 24'256);
    session.completeAudioTrackTail("track:vocal");
    if (!session.beginAudioTrackCapture("track:vocal", 1256, 24'000)) return false;
    const std::array<const float*, 2> vocalProcessedSecond{vocalLeft.data() + 256,
                                                           vocalRight.data() + 256};
    session.writeAudioTrack("track:vocal", vocalRaw.data() + 256, 256, vocalProcessedSecond.data(),
                            256);
    session.endAudioTrackCapture("track:vocal", 1512, 24'256);
    session.completeAudioTrackTail("track:vocal");

    session.writeMidiTrack("track:keys", "midi:keyboard",
                           juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)), 1100);
    session.writeMidiTrack("track:keys", "midi:keyboard",
                           juce::MidiMessage::noteOn(1, 61, static_cast<juce::uint8>(100)), 900);
    session.writeMidiTrack("track:keys", "midi:keyboard",
                           juce::MidiMessage::noteOn(1, 62, static_cast<juce::uint8>(100)), 1600);
    session.markLoopBoundary(1256);
    return session.finish(error);
}

class ArrangeRecordingSessionTest : public testing::Test {
protected:
    test::TemporaryDirectory directory;
};

}  // namespace

TEST_F(ArrangeRecordingSessionTest, CreatesTrackRecordingFilesAndManifest) {
    juce::String error;
    const auto configuration = makeConfiguration();
    auto session = ArrangeRecordingSession::create(directory.get(), configuration, error);
    ASSERT_NE(session, nullptr) << error;

    auto manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    ASSERT_TRUE(manifest.isObject());
    if (auto* object = manifest.getDynamicObject()) {
        auto* capture = new juce::DynamicObject();
        capture->setProperty("captureId", "capture:test");
        object->setProperty("capture", juce::var(capture));
        ASSERT_TRUE(directory.get()
                        .getChildFile("manifest.json")
                        .replaceWithText(juce::JSON::toString(manifest, true)));
    }

    ASSERT_TRUE(writeCapture(*session, error)) << error;
    manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    ASSERT_TRUE(manifest.isObject());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
    EXPECT_EQ(manifest.getProperty("capture", {}).getProperty("captureId", {}).toString(),
              "capture:test");
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("samplesWritten", -1)), 512);

    const auto captureSegments = manifest.getProperty("captureSegments", {});
    ASSERT_TRUE(captureSegments.isArray());
    ASSERT_EQ(captureSegments.size(), 2);
    EXPECT_EQ(static_cast<juce::int64>(captureSegments[0].getProperty("fileStartSample", -1)), 0);
    EXPECT_EQ(static_cast<juce::int64>(captureSegments[0].getProperty("fileEndSample", -1)), 256);
    EXPECT_EQ(static_cast<juce::int64>(captureSegments[1].getProperty("fileStartSample", -1)), 256);
    EXPECT_EQ(static_cast<juce::int64>(captureSegments[1].getProperty("fileEndSample", -1)), 512);

    const auto tracks = manifest.getProperty("tracks", {});
    ASSERT_TRUE(tracks.isArray());
    ASSERT_EQ(tracks.size(), 3);
    EXPECT_EQ(tracks[0].getProperty("trackId", {}).toString(), "track:guitar");
    EXPECT_EQ(tracks[0].getProperty("trackKey", {}).toString(), "0000");
    EXPECT_EQ(tracks[0].getProperty("rawFile", {}).toString(), "tracks/0000/raw.wav");
    EXPECT_EQ(tracks[1].getProperty("trackId", {}).toString(), "track:vocal");
    EXPECT_EQ(tracks[1].getProperty("trackKey", {}).toString(), "0001");
    EXPECT_EQ(tracks[2].getProperty("trackId", {}).toString(), "track:keys");
    EXPECT_EQ(tracks[2].getProperty("trackKey", {}).toString(), "0002");

    EXPECT_TRUE(directory.get().getChildFile("tracks/0000/raw.wav").existsAsFile());
    EXPECT_TRUE(directory.get().getChildFile("tracks/0000/processed.wav").existsAsFile());
    EXPECT_TRUE(directory.get().getChildFile("tracks/0001/raw.wav").existsAsFile());
    EXPECT_TRUE(directory.get().getChildFile("tracks/0001/processed.wav").existsAsFile());
    EXPECT_TRUE(directory.get().getChildFile("tracks/0002/midi.json").existsAsFile());

    const auto midi = test::parseJsonFile(directory.get().getChildFile("tracks/0002/midi.json"));
    ASSERT_TRUE(midi.isObject());
    const auto events = midi.getProperty("events", {});
    ASSERT_TRUE(events.isArray());
    ASSERT_EQ(events.size(), 2);
    EXPECT_EQ(static_cast<juce::int64>(events[0].getProperty("sampleOffset", -1)), 100);
    EXPECT_EQ(static_cast<juce::int64>(events[1].getProperty("sampleOffset", -1)), 256);
    EXPECT_EQ(static_cast<int>(events[1].getProperty("status", -1)), 128);
    EXPECT_EQ(static_cast<int>(events[1].getProperty("data1", -1)), 60);
}

TEST_F(ArrangeRecordingSessionTest, CancelsWithoutLeavingARecoverableDirectory) {
    const auto cancelledDirectory = directory.get().getChildFile("cancelled");
    juce::String error;
    auto session = ArrangeRecordingSession::create(cancelledDirectory, makeConfiguration(), error);
    ASSERT_NE(session, nullptr) << error;
    ASSERT_TRUE(session->cancel(error)) << error;

    EXPECT_FALSE(cancelledDirectory.exists());
}

}  // namespace riffra
