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

TEST(TimelineEngineTest, StoppedSeeksMoveCursorWithoutResettingPlaybackState) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(makeBuiltInInstrumentSnapshot("track:stopped-seek"), formats,
                                    48'000.0, 32, error))
        << error.toStdString();

    InstrumentTrace trace;
    auto instrument = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                                  48'000.0, 32, error);
    ASSERT_NE(instrument, nullptr) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:stopped-seek",
                                                               std::move(instrument)));

    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    const auto outputPeak = [&] {
        return std::max(std::max(std::abs(*std::max_element(left.begin(), left.end())),
                                 std::abs(*std::min_element(left.begin(), left.end()))),
                        std::max(std::abs(*std::max_element(right.begin(), right.end())),
                                 std::abs(*std::min_element(right.begin(), right.end()))));
    };

    // Act: model repeated ruler clicks while the transport remains stopped.
    ASSERT_EQ(engine.status().getProperty("state", {}).toString(), "stopped");
    ASSERT_TRUE(engine.enqueueTargetedMidi("track:stopped-seek",
                                           juce::MidiMessage::noteOn(1, 60, 0.8f), error));
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    ASSERT_TRUE(trace.noteHeld);
    const auto discontinuityBeforeSeeks =
        static_cast<juce::int64>(engine.status().getProperty("discontinuity", -1));
    for (std::uint64_t index = 1; index <= 50; ++index) engine.seekToTick(index * 10);
    engine.seekToTick(0);
    const auto statusBeforeMix = engine.status();
    left.fill(0.0f);
    right.fill(0.0f);
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto statusAfterMix = engine.status();

    // Assert: the cursor moves immediately and the stopped callback does not
    // turn the cursor move into a playback discontinuity.
    EXPECT_EQ(static_cast<juce::int64>(statusBeforeMix.getProperty("timelineSample", -1)), 0);
    EXPECT_EQ(static_cast<juce::int64>(statusAfterMix.getProperty("timelineSample", -1)), 0);
    EXPECT_EQ(trace.transportResetBlocks, 0);
    EXPECT_TRUE(trace.noteHeld);
    EXPECT_GT(outputPeak(), 0.0f);
    EXPECT_EQ(static_cast<juce::int64>(statusAfterMix.getProperty("discontinuity", -1)),
              discontinuityBeforeSeeks);

    engine.play();
    left.fill(0.0f);
    right.fill(0.0f);
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

    const auto playingStatus = engine.status();
    EXPECT_EQ(playingStatus.getProperty("state", {}).toString(), "playing");
    EXPECT_EQ(static_cast<juce::int64>(playingStatus.getProperty("timelineSample", -1)), 32);
    EXPECT_GT(outputPeak(), 0.0f);
    EXPECT_EQ(trace.transportResetBlocks, 0);
}

TEST(TimelineEngineTest, PlayingSeekUsesCallbackBoundaryAndResetsPlaybackState) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.loadSnapshot(makeBuiltInInstrumentSnapshot("track:playing-seek"), formats,
                                    48'000.0, 32, error))
        << error.toStdString();

    InstrumentTrace trace;
    auto instrument = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                                  48'000.0, 32, error);
    ASSERT_NE(instrument, nullptr) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:playing-seek",
                                                               std::move(instrument)));

    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    engine.play();
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto sampleBeforeSeek =
        static_cast<juce::int64>(engine.status().getProperty("timelineSample", -1));
    const auto resetBlocksBeforeSeek = trace.transportResetBlocks;
    const auto discontinuityBeforeSeek =
        static_cast<juce::int64>(engine.status().getProperty("discontinuity", -1));

    // Act
    engine.seekToTick(480);
    const auto statusBeforeCallback = engine.status();
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto statusAfterCallback = engine.status();

    // Assert: the request is applied at the callback boundary, then playback
    // continues from the requested sample after the reset is observed.
    EXPECT_EQ(static_cast<juce::int64>(statusBeforeCallback.getProperty("timelineSample", -1)),
              sampleBeforeSeek);
    EXPECT_EQ(static_cast<juce::int64>(statusAfterCallback.getProperty("timelineSample", -1)),
              12'032);
    EXPECT_GT(trace.transportResetBlocks, resetBlocksBeforeSeek);
    EXPECT_GT(static_cast<juce::int64>(statusAfterCallback.getProperty("discontinuity", -1)),
              discontinuityBeforeSeek);
    EXPECT_GT(static_cast<juce::int64>(statusAfterCallback.getProperty("audioClockSample", -1)),
              sampleBeforeSeek);
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

}  // namespace riffra
