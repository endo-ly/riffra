#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, ProcessesAnInstrumentRuntimeOncePerTransportChunk) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, makeInstrumentSnapshot("track:live-fade"), formats,
                                 48'000.0, kBlockSamples, error))
        << error.toStdString();
    InstrumentTrace trace;
    auto instrument = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, kBlockSamples, error);
    ASSERT_NE(instrument, nullptr) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(
        engine, "track:live-fade", "instrument:live-fade", std::move(instrument)));
    ASSERT_TRUE((engine.setLiveMidiTarget("track:live-fade", error) == RealtimeRequest::accepted));
    ASSERT_TRUE(
        (engine.enqueueTargetedMidi("track:live-fade", juce::MidiMessage::noteOn(1, 60, 0.8f),
                                    error) == RealtimeRequest::accepted));

    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.play());
    engine.mix(outputs.data(), 2, kBlockSamples);

    // Assert: the 5 ms fade splits the callback into two ranges, but the
    // stateful Instrument Runtime is advanced once for each range.
    EXPECT_EQ(trace.processBlockCount, 2);
}

TEST(TimelineEngineTest, ProcessesAnAudioEffectChainOncePerTransportChunk) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, true, false), formats, 48'000.0,
                                 kBlockSamples, error))
        << error.toStdString();
    ProcessorTrace trace;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:live", "effect:live-fade", std::make_unique<TestProcessor>(trace), 48'000.0,
        kBlockSamples, error))
        << error.toStdString();

    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.play());
    engine.mix(inputs.data(), 1, outputs.data(), 2, kBlockSamples);

    // Assert: the live monitor input is merged before the shared effect chain,
    // so the 5 ms fade still advances that stateful chain once per range.
    EXPECT_EQ(trace.processBlockCount, 2);
}

TEST(TimelineEngineTest, LiveMidiTailIncludesEffectChainTail) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeInstrumentSnapshot("track:tail"), formats, 48'000.0,
                                 32, error));
    InstrumentTrace instrumentTrace;
    auto instrument = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(instrumentTrace), 48'000.0, 32, error);
    ASSERT_NE(instrument, nullptr) << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(
        engine, "track:tail", "instrument:tail", std::move(instrument)));
    std::vector<int> effectCalls;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackChainDevice(
        engine, "track:tail", "effect:tail",
        std::make_unique<TestChainProcessor>(1, 1.0f, 0, effectCalls, 0.25), 48'000.0, 32, error))
        << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::cachePluginTailForTest(engine, "track:tail"));

    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    ASSERT_TRUE((engine.enqueueTargetedMidi("track:tail", juce::MidiMessage::noteOn(1, 60, 0.8f),
                                            error) == RealtimeRequest::accepted));
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
    const auto callsWithNoteHeld = effectCalls.size();

    // Act
    ASSERT_TRUE((engine.enqueueTargetedMidi("track:tail", juce::MidiMessage::noteOff(1, 60),
                                            error) == RealtimeRequest::accepted));
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
    ASSERT_TRUE(loadTestSnapshot(engine, makeBuiltInInstrumentSnapshot("track:builtin", true, true),
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
    ASSERT_TRUE(engine.play());
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto timelinePeak = outputMagnitude();
    auto loopPeak = 0.0f;
    for (int block = 0; block < 16; ++block) {
        clearOutput();
        engine.mix(outputChannels.data(), 2, kBlockSamples);
        loopPeak = std::max(loopPeak, outputMagnitude());
    }

    // A seek must reset the built-in runtime before the next timeline note.
    ASSERT_TRUE(engine.seekToTick(0));
    ASSERT_TRUE(engine.play());
    clearOutput();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto seekPeak = outputMagnitude();

    // Stopped transport still accepts targeted live MIDI, including directly
    // after stop/reset.
    ASSERT_TRUE(engine.stop());
    ASSERT_TRUE((engine.enqueueTargetedMidi("track:builtin", juce::MidiMessage::noteOn(1, 64, 0.8f),
                                            error) == RealtimeRequest::accepted))
        << error.toStdString();
    clearOutput();
    engine.mix(outputChannels.data(), 2, kBlockSamples);
    const auto livePeak = outputMagnitude();

    ASSERT_TRUE(engine.stop());
    ASSERT_TRUE((engine.enqueueTargetedMidi("track:builtin", juce::MidiMessage::noteOn(1, 67, 0.7f),
                                            error) == RealtimeRequest::accepted))
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
    const auto status = engine.status();
    ASSERT_TRUE(status.graph.has_value());
    EXPECT_EQ(status.graph->armedTrackIds.size(), 1u);
}

TEST(TimelineEngineTest, MonitorsAudioTrackInputWhileTransportIsStopped) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;

    auto snapshot = makeTestSnapshot();
    for (const auto& [id, muted] : std::array<std::pair<const char*, bool>, 2>{
             std::pair{"track:guitar", false}, std::pair{"track:muted-guitar", true}}) {
        auto track = makeAudioTrack(id);
        track.muted = muted;
        track.monitoring = MonitoringSpec::on;
        track.audioInput = AudioInputSpec{0};
        snapshot.graph.tracks.push_back(std::move(track));
    }
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error));

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

    ASSERT_TRUE(engine.play());
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
        if (!loadTestSnapshot(engine, makeAudioTrackSnapshot(trackCount, true, false), formats,
                              48'000.0, kBlockSamples, error))
            return false;
        std::array<float, kBlockSamples> input{};
        input.fill(0.05f);
        std::array<float, kBlockSamples> outputLeft{};
        std::array<float, kBlockSamples> outputRight{};
        const std::array<const float*, 1> inputChannels{input.data()};
        const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};

        if (!engine.play()) return false;
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
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile, 1024);

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error))
        << error.toStdString();
    constexpr int kBlockSamples = 64;
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act: play from a non-zero seek position and stop while the source is loud.
    ASSERT_TRUE(engine.seekToTick(4));
    ASSERT_TRUE(engine.play());
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto playStartPeak = *std::max_element(left.begin(), left.end());
    EXPECT_LT(left.front(), 0.01f);
    EXPECT_GT(playStartPeak, 0.015f);

    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto previous = left.back();
    ASSERT_TRUE(engine.stop());
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

TEST(TimelineEngineTest, MetronomeStopUsesTheTransportFadeBoundary) {
    // Arrange
    auto snapshot = makeTestSnapshot();
    snapshot.graph.metronomeEnabled = true;

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, kBlockSamples, error))
        << error.toStdString();
    std::array<float, kBlockSamples> output{};
    const std::array<float*, 1> outputs{output.data()};

    // Act
    ASSERT_TRUE(engine.play());
    engine.mix(outputs.data(), 1, kBlockSamples);
    ASSERT_TRUE(engine.stop());
    output.fill(0.0f);
    engine.mix(outputs.data(), 1, kBlockSamples);
    engine.mixMetronome(outputs.data(), 1, kBlockSamples);

    // Assert: the click continues through the 240-sample fade and then stays
    // silent for the remainder of the callback.
    EXPECT_GT(output.front(), 0.05f);
    EXPECT_LT(std::abs(output[239]), 0.001f);
    EXPECT_LT(*std::max_element(output.begin() + 240, output.end()), 0.001f);
}

TEST(TimelineEngineTest, TransportPlayFromTimelineZeroUsesTheDeclickEnvelope) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("play-zero-raw.wav");
    const auto processedFile = directory.get().getChildFile("play-zero-processed.wav");
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, 1024, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, 1024, 3'277));
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile, 1024);

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error))
        << error.toStdString();
    std::array<float, 64> left{};
    std::array<float, 64> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.play());
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
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile, 1024);

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine(true);
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, kBlockSamples, error))
        << error.toStdString();
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.seekToTick(4));
    ASSERT_TRUE(engine.play());
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto beforeStop = engine.status().frame.timelineSample;
    ASSERT_TRUE(engine.stop());
    std::fill(left.begin(), left.end(), 0.0f);
    std::fill(right.begin(), right.end(), 0.0f);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto afterStop = engine.status().frame.timelineSample;

    // Assert: the playhead advances by the 5 ms fade only, not by the 512
    // sample callback, and remains stable on subsequent stopped callbacks.
    EXPECT_EQ(afterStop - beforeStop, 240);
    EXPECT_GT(left.front(), 0.01f);
    EXPECT_LT(std::abs(left.back()), 0.001f);
    engine.mix(outputs.data(), 2, kBlockSamples);
    EXPECT_EQ(engine.status().frame.timelineSample, afterStop);
}

}  // namespace riffra
