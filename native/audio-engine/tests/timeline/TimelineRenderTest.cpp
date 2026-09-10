#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

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

TEST(TimelineEngineTest, MergesMonitoredInputBeforeTrackProcessing) {
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
    // Audio monitoring bypasses only inter-track compensation delay.
    EXPECT_GT(left[0], 0.23f);
    EXPECT_NEAR(right[0], left[0], 0.002f);
    EXPECT_NEAR(left[4], left[0], 0.002f);
    EXPECT_NEAR(right[4], left[4], 0.002f);
}

TEST(TimelineEngineTest, ProcessesEachTrackEffectChainOnce) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::trackEffectChainProcessesOnce();

    // Assert
    EXPECT_TRUE(passed);
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
