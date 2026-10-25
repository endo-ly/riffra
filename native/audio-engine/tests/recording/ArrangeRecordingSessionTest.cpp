#include <gtest/gtest.h>

#include <array>

#include "../support/TestSupport.h"
#include "recording/ArrangeRecordingSession.h"

namespace riffra {
namespace {

RecordingConfiguration makeConfiguration() {
    RecordingConfiguration configuration;
    configuration.sampleRate = 48'000.0;
    configuration.timelineStartTick = 960;
    configuration.loopEnabled = true;
    configuration.loopStartSample = 24'000;
    configuration.loopEndSample = 48'000;
    configuration.punchEnabled = false;

    const auto addTrack = [&configuration](const juce::String& id, TrackKindSpec kind, int input) {
        RecordingTrackConfiguration track;
        track.trackId = id;
        track.kind = kind;
        track.audioInputChannel = input;
        track.pluginLatencySamples = input + 8;
        track.pluginTailSamples = input + 16;
        configuration.tracks.push_back(std::move(track));
    };
    addTrack("track:guitar", TrackKindSpec::audio, 0);
    addTrack("track:vocal", TrackKindSpec::audio, 1);
    addTrack("track:keys", TrackKindSpec::instrument, -1);

    return configuration;
}

bool writeCapture(ArrangeRecordingSession& session, juce::String& error) {
    std::array<float, 512> guitarRaw{};
    std::array<float, 512> guitarProcessedLeft{};
    std::array<float, 512> guitarProcessedRight{};
    std::array<float, 512> vocalRaw{};
    std::array<float, 512> vocalProcessedLeft{};
    std::array<float, 512> vocalProcessedRight{};
    guitarRaw.fill(0.1f);
    guitarProcessedLeft.fill(0.2f);
    guitarProcessedRight.fill(0.21f);
    vocalRaw.fill(0.3f);
    vocalProcessedLeft.fill(0.4f);
    vocalProcessedRight.fill(0.41f);
    const std::array<const float*, 2> guitarProcessed{guitarProcessedLeft.data(),
                                                      guitarProcessedRight.data()};
    const std::array<const float*, 2> vocalProcessed{vocalProcessedLeft.data(),
                                                     vocalProcessedRight.data()};

    session.setCaptureRange(1000, 1256, 24'000, 24'256);
    session.setCaptureRange(1256, 1512, 24'000, 24'256);
    if (!session.beginAudioTrackCapture("track:guitar", 1000, 24'000)) return false;
    session.writeAudioTrack("track:guitar", guitarRaw.data(), 256);
    if (!session.writeProcessedAudioTrackOffline("track:guitar", guitarProcessed.data(), 256, 100))
        return false;
    session.endAudioTrackCapture("track:guitar", 1256, 24'256);
    if (!session.beginAudioTrackCapture("track:guitar", 1256, 24'000)) return false;
    session.writeAudioTrack("track:guitar", guitarRaw.data() + 256, 256);
    if (!session.writeProcessedAudioTrackOffline("track:guitar", guitarProcessed.data(), 256, 100))
        return false;
    session.endAudioTrackCapture("track:guitar", 1512, 24'256);

    if (!session.beginAudioTrackCapture("track:vocal", 1000, 24'000)) return false;
    session.writeAudioTrack("track:vocal", vocalRaw.data(), 256);
    if (!session.writeProcessedAudioTrackOffline("track:vocal", vocalProcessed.data(), 256, 100))
        return false;
    session.endAudioTrackCapture("track:vocal", 1256, 24'256);
    if (!session.beginAudioTrackCapture("track:vocal", 1256, 24'000)) return false;
    session.writeAudioTrack("track:vocal", vocalRaw.data() + 256, 256);
    if (!session.writeProcessedAudioTrackOffline("track:vocal", vocalProcessed.data(), 256, 100))
        return false;
    session.endAudioTrackCapture("track:vocal", 1512, 24'256);

    session.writeMidiTrack("track:keys", std::uint16_t{0},
                           juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)), 1100);
    session.writeMidiTrack("track:keys", "midi:keyboard",
                           juce::MidiMessage::noteOn(1, 61, static_cast<juce::uint8>(100)), 900);
    session.writeMidiTrack("track:keys", "midi:keyboard",
                           juce::MidiMessage::noteOn(1, 62, static_cast<juce::uint8>(100)), 1600);
    session.setMidiSourceIds({"midi:late-keyboard"});
    session.markLoopBoundary(1256);
    return session.finish(true, error);
}

class ArrangeRecordingSessionTest : public testing::Test {
protected:
    test::TemporaryDirectory directory;
};

}  // namespace

TEST_F(ArrangeRecordingSessionTest, CreatesTrackRecordingFilesAndManifest) {
    juce::String error;
    auto configuration = makeConfiguration();
    configuration.tracks[0].sidechainInputs = {"device:compressor", "device:gate"};
    configuration.tracks[2].midiDeviceId = "midi:keyboard";
    configuration.tracks[2].midiChannel = 3;
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

    std::array<float, 16> sidechainLeft;
    std::array<float, 16> sidechainRight;
    sidechainLeft.fill(0.25f);
    sidechainRight.fill(-0.5f);
    const std::array<const float*, 2> sidechain{sidechainLeft.data(), sidechainRight.data()};
    session->writeSidechain("track:guitar", "device:compressor", sidechain.data(), 16);
    const auto sidechainFile =
        session->prepareSidechainForReading("track:guitar", "device:compressor");
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    const std::unique_ptr<juce::AudioFormatReader> reader(formats.createReaderFor(sidechainFile));
    ASSERT_NE(reader, nullptr);
    EXPECT_EQ(reader->lengthInSamples, 16);
    juce::AudioBuffer<float> capturedSidechain(2, 16);
    ASSERT_TRUE(reader->read(&capturedSidechain, 0, 16, 0, true, true));
    EXPECT_FLOAT_EQ(capturedSidechain.getSample(0, 0), 0.25f);
    EXPECT_FLOAT_EQ(capturedSidechain.getSample(1, 0), -0.5f);

    ASSERT_TRUE(writeCapture(*session, error)) << error;
    manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    ASSERT_TRUE(manifest.isObject());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
    EXPECT_EQ(manifest.getProperty("capture", {}).getProperty("captureId", {}).toString(),
              "capture:test");

    EXPECT_DOUBLE_EQ(static_cast<double>(manifest.getProperty("sampleRate", 0)), 48'000.0);
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("timelineStartTick", 0)), 960);
    const auto loop = manifest.getProperty("loopRange", {});
    EXPECT_TRUE(static_cast<bool>(loop.getProperty("enabled", false)));
    EXPECT_EQ(static_cast<juce::int64>(loop.getProperty("startSample", 0)), 24'000);
    EXPECT_EQ(static_cast<juce::int64>(loop.getProperty("endSample", 0)), 48'000);

    // Arrange recording captures every Track into its own files and segments.
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
    EXPECT_EQ(tracks[0].getProperty("kind", {}).toString(), "audio");
    EXPECT_EQ(static_cast<int>(tracks[0].getProperty("pluginLatencySamples", 0)), 8);
    EXPECT_EQ(static_cast<int>(tracks[0].getProperty("pluginTailSamples", 0)), 16);
    const auto sidechainInputs = tracks[0].getProperty("sidechainInputs", {});
    ASSERT_EQ(sidechainInputs.size(), 2);
    EXPECT_EQ(sidechainInputs[0].getProperty("deviceId", {}).toString(), "device:compressor");
    EXPECT_EQ(sidechainInputs[0].getProperty("file", {}).toString(), "tracks/0000/sidechain-0.wav");
    EXPECT_EQ(sidechainInputs[1].getProperty("deviceId", {}).toString(), "device:gate");
    EXPECT_EQ(sidechainInputs[1].getProperty("file", {}).toString(), "tracks/0000/sidechain-1.wav");
    EXPECT_EQ(tracks[0].getProperty("rawFile", {}).toString(), "tracks/0000/raw.wav");
    EXPECT_EQ(tracks[1].getProperty("trackId", {}).toString(), "track:vocal");
    EXPECT_EQ(tracks[1].getProperty("trackKey", {}).toString(), "0001");
    EXPECT_EQ(tracks[2].getProperty("trackId", {}).toString(), "track:keys");
    EXPECT_EQ(tracks[2].getProperty("trackKey", {}).toString(), "0002");
    EXPECT_EQ(tracks[2].getProperty("kind", {}).toString(), "instrument");
    const auto midiInput = tracks[2].getProperty("midiInput", {});
    EXPECT_EQ(midiInput.getProperty("deviceId", {}).toString(), "midi:keyboard");
    EXPECT_EQ(static_cast<int>(midiInput.getProperty("channel", 0)), 3);

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
    EXPECT_EQ(events[0].getProperty("sourceDeviceId", {}).toString(), "midi:late-keyboard");
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

TEST_F(ArrangeRecordingSessionTest, FailedProcessingKeepsRawAudioRecoverable) {
    juce::String error;
    auto session = ArrangeRecordingSession::create(directory.get(), makeConfiguration(), error);
    ASSERT_NE(session, nullptr) << error;

    std::array<float, 512> raw{};
    ASSERT_TRUE(session->beginAudioTrackCapture("track:guitar", 1000, 24'000));
    session->writeAudioTrack("track:guitar", raw.data(), static_cast<int>(raw.size()));
    ASSERT_TRUE(session->endAudioTrackCapture("track:guitar", 1512, 24'512));

    EXPECT_FALSE(session->finish(false, error));
    const auto manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "recoverable");
    EXPECT_EQ(manifest.getProperty("tracks", {})[0].getProperty("rawFile", {}).toString(),
              "tracks/0000/raw.wav");
    EXPECT_EQ(manifest.getProperty("tracks", {})[0].getProperty("processedFile", {}).toString(),
              "tracks/0000/processed.wav.partial");
    EXPECT_TRUE(directory.get().getChildFile("tracks/0000/raw.wav").existsAsFile());
    EXPECT_FALSE(directory.get().getChildFile("tracks/0000/processed.wav").existsAsFile());
}

TEST_F(ArrangeRecordingSessionTest, ReportsOversizedMidiSourceIds) {
    juce::String error;
    auto session = ArrangeRecordingSession::create(directory.get(), makeConfiguration(), error);
    ASSERT_NE(session, nullptr) << error;

    std::array<float, 1> raw{0.1f};
    std::array<float, 1> processedLeft{0.2f};
    std::array<float, 1> processedRight{0.21f};
    const std::array<const float*, 2> processed{processedLeft.data(), processedRight.data()};
    ASSERT_TRUE(session->beginAudioTrackCapture("track:guitar", 1000, 24'000));
    session->writeAudioTrack("track:guitar", raw.data(), 1);
    ASSERT_TRUE(session->writeProcessedAudioTrackOffline("track:guitar", processed.data(), 1, 100));
    ASSERT_TRUE(session->endAudioTrackCapture("track:guitar", 1001, 24'001));
    ASSERT_TRUE(session->beginAudioTrackCapture("track:vocal", 1000, 24'000));
    session->writeAudioTrack("track:vocal", raw.data(), 1);
    ASSERT_TRUE(session->writeProcessedAudioTrackOffline("track:vocal", processed.data(), 1, 100));
    ASSERT_TRUE(session->endAudioTrackCapture("track:vocal", 1001, 24'001));

    juce::String oversizedSource;
    for (int index = 0; index < 65; ++index) oversizedSource << "x";
    session->writeMidiTrack("track:keys", oversizedSource, juce::MidiMessage::noteOn(1, 60, 1.0f),
                            1100);

    EXPECT_EQ(session->droppedMidiEvents(), 1u);
    EXPECT_EQ(session->summary().droppedMidiEvents, 1u);

    EXPECT_TRUE(session->finish(true, error)) << error;
    const auto manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("droppedMidiEvents", -1)), 1);
    EXPECT_EQ(manifest.getProperty("recoveryStatus", {}).toString(), "partial");
}

}  // namespace riffra
