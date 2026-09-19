#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

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

TEST(TimelineEngineTest, TransportBoundariesConvergeWithoutAOneSampleCut) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("transport-raw.wav");
    const auto processedFile = directory.get().getChildFile("transport-processed.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 1024, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 1024, 3'277));
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile);
    auto* snapshotObject = snapshot.getDynamicObject();
    ASSERT_NE(snapshotObject, nullptr);
    auto tracks = snapshotObject->getProperty("tracks");
    ASSERT_TRUE(tracks.isArray() && tracks.size() == 1);
    auto* track = tracks[0].getDynamicObject();
    ASSERT_NE(track, nullptr);
    auto clips = track->getProperty("audioClips");
    ASSERT_TRUE(clips.isArray());
    for (auto& clipValue : *clips.getArray()) {
        auto* clip = clipValue.getDynamicObject();
        ASSERT_NE(clip, nullptr);
        clip->setProperty("sourceEndFrame", 1024);
        clip->setProperty("durationFrames", 1024);
    }

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(snapshot, formats, 48'000.0, 64, error)) << error.toStdString();
    constexpr int kBlockSamples = 64;
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act: play from a non-zero seek position and stop while the source is loud.
    engine.seekToTick(4);
    engine.play();
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto playStartPeak = *std::max_element(left.begin(), left.end());
    EXPECT_LT(left.front(), 0.01f);
    EXPECT_GT(playStartPeak, 0.015f);

    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto previous = left.back();
    engine.stop();
    std::fill(left.begin(), left.end(), 0.0f);
    std::fill(right.begin(), right.end(), 0.0f);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto stopFirst = left.front();
    const auto stopLast = left.back();
    const auto stopStep = std::abs(stopFirst - stopLast);

    // Assert: both transitions remain audible for the short de-click and then settle.
    EXPECT_GT(previous, 0.02f);
    EXPECT_GT(stopFirst, 0.02f);
    EXPECT_GT(stopLast, 0.02f);
    EXPECT_LT(stopStep, 0.02f);
    for (int block = 0; block < 16; ++block) {
        std::fill(left.begin(), left.end(), 0.0f);
        std::fill(right.begin(), right.end(), 0.0f);
        engine.mix(outputs.data(), 2, kBlockSamples);
    }
    EXPECT_LT(std::abs(left.back()), 0.001f);
}

TEST(TimelineEngineTest, TransportPlayFromTimelineZeroUsesTheDeclickEnvelope) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("play-zero-raw.wav");
    const auto processedFile = directory.get().getChildFile("play-zero-processed.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 1024, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 1024, 3'277));
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile);
    auto* snapshotObject = snapshot.getDynamicObject();
    ASSERT_NE(snapshotObject, nullptr);
    auto tracks = snapshotObject->getProperty("tracks");
    ASSERT_TRUE(tracks.isArray() && tracks.size() == 1);
    auto* track = tracks[0].getDynamicObject();
    ASSERT_NE(track, nullptr);
    auto clips = track->getProperty("audioClips");
    ASSERT_TRUE(clips.isArray());
    for (auto& clipValue : *clips.getArray()) {
        auto* clip = clipValue.getDynamicObject();
        ASSERT_NE(clip, nullptr);
        clip->setProperty("sourceEndFrame", 1024);
        clip->setProperty("durationFrames", 1024);
    }

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(snapshot, formats, 48'000.0, 64, error)) << error.toStdString();
    std::array<float, 64> left{};
    std::array<float, 64> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    engine.play();
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

    // Assert: a non-zero first source sample still starts at zero gain and
    // reaches audible level within the short transport fade.
    EXPECT_NEAR(left.front(), 0.0f, 0.001f);
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.005f);
}

TEST(TimelineEngineTest, TransportStopAdvancesOnlyThroughTheFadeBoundary) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("stop-block-raw.wav");
    const auto processedFile = directory.get().getChildFile("stop-block-processed.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 1024, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 1024, 3'277));
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile);
    auto* snapshotObject = snapshot.getDynamicObject();
    ASSERT_NE(snapshotObject, nullptr);
    auto tracks = snapshotObject->getProperty("tracks");
    ASSERT_TRUE(tracks.isArray() && tracks.size() == 1);
    auto* track = tracks[0].getDynamicObject();
    ASSERT_NE(track, nullptr);
    auto clips = track->getProperty("audioClips");
    ASSERT_TRUE(clips.isArray());
    for (auto& clipValue : *clips.getArray()) {
        auto* clip = clipValue.getDynamicObject();
        ASSERT_NE(clip, nullptr);
        clip->setProperty("sourceEndFrame", 1024);
        clip->setProperty("durationFrames", 1024);
    }

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(engine.loadSnapshot(snapshot, formats, 48'000.0, kBlockSamples, error))
        << error.toStdString();
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    engine.seekToTick(4);
    engine.play();
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto beforeStop = static_cast<std::int64_t>(
        engine.status().getProperty("timelineSample", static_cast<juce::int64>(-1)));
    engine.stop();
    std::fill(left.begin(), left.end(), 0.0f);
    std::fill(right.begin(), right.end(), 0.0f);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto afterStop = static_cast<std::int64_t>(
        engine.status().getProperty("timelineSample", static_cast<juce::int64>(-1)));

    // Assert: the playhead advances by the 5 ms fade only, not by the 512
    // sample callback, and remains stable on subsequent stopped callbacks.
    EXPECT_EQ(afterStop - beforeStop, 240);
    EXPECT_GT(left.front(), 0.01f);
    EXPECT_LT(std::abs(left.back()), 0.001f);
    engine.mix(outputs.data(), 2, kBlockSamples);
    EXPECT_EQ(static_cast<std::int64_t>(
                  engine.status().getProperty("timelineSample", static_cast<juce::int64>(-1))),
              afterStop);
}

}  // namespace riffra
