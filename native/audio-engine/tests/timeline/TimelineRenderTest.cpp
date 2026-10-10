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
    constexpr int kSourceFrames = 512;
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, kSourceFrames, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, kSourceFrames, 3'277));

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile, kSourceFrames);
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error))
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
    ASSERT_TRUE(engine.play());
    for (int block = 0; block < 8; ++block) {
        std::fill(left.begin(), left.end(), 0.0f);
        std::fill(right.begin(), right.end(), 0.0f);
        engine.mix(outputChannels.data(), 2, kBlockSamples);
    }
    const auto expected = (0.05f * 2.0f + 0.10f) * 0.5f;

    // Assert
    ASSERT_FALSE(processOrder.empty());
    EXPECT_TRUE(std::all_of(processOrder.begin(), processOrder.end(),
                            [](const int id) { return id == 1; }));
    EXPECT_NEAR(left[20], expected, 0.002f);
    EXPECT_NEAR(right[20], expected, 0.002f);
    EXPECT_NEAR(left[31], expected, 0.002f);
    EXPECT_NEAR(right[31], expected, 0.002f);

    // Act: a transport discontinuity must clear both compensation lines.
    ASSERT_TRUE(engine.stop());
    ASSERT_TRUE(engine.seekToTick(0));
    std::fill(left.begin(), left.end(), 0.0f);
    std::fill(right.begin(), right.end(), 0.0f);
    ASSERT_TRUE(engine.play());
    for (int block = 0; block < 8; ++block) {
        std::fill(left.begin(), left.end(), 0.0f);
        std::fill(right.begin(), right.end(), 0.0f);
        engine.mix(outputChannels.data(), 2, kBlockSamples);
    }

    // Assert
    EXPECT_NEAR(left[20], expected, 0.002f);
    EXPECT_NEAR(right[20], expected, 0.002f);
}

TEST(TimelineEngineTest, AppliesTrackMixPreviewToOutputAndTrackMeter) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, true, false), formats, 48'000.0,
                                 32, error))
        << error.toStdString();

    constexpr int kBlockSamples = 32;
    std::array<float, kBlockSamples> input{};
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    const auto mixLiveInput = [&] {
        left.fill(0.0f);
        right.fill(0.0f);
        engine.mix(inputs.data(), 1, outputs.data(), 2, kBlockSamples);
    };

    // Act: measure the canonical center-panned output, then apply a transient
    // gain/pan change without publishing a new snapshot.
    mixLiveInput();
    const auto baselineLeft = juce::FloatVectorOperations::findMaximum(left.data(), kBlockSamples);
    const auto baselineRight =
        juce::FloatVectorOperations::findMaximum(right.data(), kBlockSamples);
    const auto baselineMeters = engine.meterSnapshot();
    ASSERT_EQ(baselineMeters.size(), 1u);
    EXPECT_NEAR(static_cast<float>(baselineMeters[0].peakLeft), baselineLeft, 0.01f);
    EXPECT_NEAR(static_cast<float>(baselineMeters[0].peakRight), baselineRight, 0.01f);
    EXPECT_GT(static_cast<float>(baselineMeters[0].rmsLeft), 0.0f);
    EXPECT_GT(static_cast<float>(baselineMeters[0].rmsRight), 0.0f);

    const auto beforePreviewStatus = engine.status();
    ASSERT_TRUE(beforePreviewStatus.graph.has_value());

    ASSERT_TRUE(engine.setTrackMixControl("track:live", -6.0f, -1.0f, error))
        << error.toStdString();
    const auto afterPreviewStatus = engine.status();
    ASSERT_TRUE(afterPreviewStatus.graph.has_value());
    EXPECT_EQ(afterPreviewStatus.graph->revision, beforePreviewStatus.graph->revision);
    EXPECT_EQ(afterPreviewStatus.frame.graphPublishCount,
              beforePreviewStatus.frame.graphPublishCount);
    mixLiveInput();
    const auto previewMeters = engine.meterSnapshot();

    // Assert: preview is audible immediately, follows the final pan, and the
    // meter observes the same post-fader/post-pan contribution.
    ASSERT_EQ(previewMeters.size(), 1u);
    EXPECT_GT(left[0], right[0]);
    EXPECT_LT(right[0], 0.001f);
    EXPECT_GT(static_cast<float>(previewMeters[0].peakLeft), 0.0f);
    EXPECT_LT(static_cast<float>(previewMeters[0].peakRight), 0.001f);

    // Act: publish a fresh canonical snapshot and render again.
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, true, false), formats, 48'000.0,
                                 32, error))
        << error.toStdString();
    mixLiveInput();

    // Assert: a graph publication starts from the canonical center-panned
    // value instead of carrying the transient preview forward.
    EXPECT_NEAR(left[0], right[0], 0.01f);
}

TEST(TimelineEngineTest, TrackAutomationOverridesStaticMixPreview) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, true, false);
    snapshot.graph.tracks.front().volumeAutomation.push_back({0, -12.0});

    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error))
        << error.toStdString();
    std::array<float, 32> input{};
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act: render the automation value, then preview a different static gain.
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(input.size()));
    const auto automatedPeak =
        juce::FloatVectorOperations::findMaximum(left.data(), static_cast<int>(left.size()));
    ASSERT_TRUE(engine.setTrackMixControl("track:live", -24.0f, std::nullopt, error))
        << error.toStdString();
    left.fill(0.0f);
    right.fill(0.0f);
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(input.size()));

    // Assert: the absolute automation value remains authoritative.
    EXPECT_NEAR(
        juce::FloatVectorOperations::findMaximum(left.data(), static_cast<int>(left.size())),
        automatedPeak, 0.01f);
}

TEST(TimelineEngineTest, TrackMetersExcludeNonAudibleSoloTracks) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(2, true, false);
    snapshot.graph.tracks[1].monitorInput = true;
    snapshot.graph.tracks[1].solo = true;

    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error))
        << error.toStdString();
    std::array<float, 32> input{};
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(input.size()));
    const auto meters = engine.meterSnapshot();

    // Assert
    ASSERT_EQ(meters.size(), 2u);
    EXPECT_FLOAT_EQ(static_cast<float>(meters[0].peakLeft), 0.0f);
    EXPECT_GT(static_cast<float>(meters[1].peakLeft), 0.0f);
}

TEST(TimelineEngineTest, MergesMonitoredInputBeforeTrackProcessing) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto rawFile = directory.get().getChildFile("raw-monitor.wav");
    const auto processedFile = directory.get().getChildFile("processed-monitor.wav");
    constexpr int kSourceFrames = 512;
    ASSERT_TRUE(writePcmWave(rawFile, 48'000, 1, kSourceFrames, 1'638));
    ASSERT_TRUE(writePcmWave(processedFile, 48'000, 1, kSourceFrames, 3'277));

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeRawAndProcessedClipSnapshot(rawFile, processedFile, kSourceFrames);
    snapshot.graph.tracks.front().monitorInput = true;
    snapshot.graph.tracks.front().audioInput = AudioInputSpec{0};

    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error))
        << error.toStdString();
    ASSERT_TRUE(TimelineEngineTestPeer::setPlaybackCompensationForTest(engine, "track:audio", 4));

    std::array<float, 32> inputSamples{};
    inputSamples.fill(0.25f);
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<const float*, 1> inputs{inputSamples.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.play());
    for (int block = 0; block < 8; ++block) {
        std::fill(left.begin(), left.end(), 0.0f);
        std::fill(right.begin(), right.end(), 0.0f);
        engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(left.size()));
    }

    // Assert
    // Audio monitoring bypasses only inter-track compensation delay.
    EXPECT_GT(left[0], 0.22f);
    EXPECT_NEAR(right[0], left[0], 0.01f);
    EXPECT_NEAR(left[4], left[0], 0.01f);
    EXPECT_NEAR(right[4], left[4], 0.01f);
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
    OfflineRenderer::Result result;
    juce::String error;

    // Act
    const auto snapshot = makeBuiltInInstrumentSnapshot("track:offline");
    const auto rendered = renderTestSnapshot(snapshot.graph, formats, destination, 0, 960, 48'000,
                                             512, false, result, error);

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

    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:guitar");
    track.monitorInput = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error));
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
    ASSERT_TRUE(engine.seekToTick(0));
    ASSERT_TRUE(engine.play());
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
    ASSERT_GE(processOrder.size(), 2u);
    EXPECT_TRUE(std::all_of(processOrder.begin(), processOrder.end(),
                            [](const int id) { return id == 1; }));
}

TEST(TimelineEngineTest, RoutesOneMutedSourceToParallelConsumersAndRejectsCycles) {
    // Arrange: put Consumers before their Source in presentation order.
    test::TemporaryDirectory directory;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeBuiltInInstrumentSnapshot("source");
    auto source = snapshot.graph.tracks.front();
    source.muted = true;
    source.panLaw = "unityCenterStereo";
    const auto instrumentDirectory = juce::File(RIFFRA_CONTRACT_FIXTURE_DIR)
                                         .getSiblingFile("sonalloy-bundle")
                                         .getChildFile("basic/instruments/duck");
    auto consumer = makeInstrumentTrack("consumer-a");
    consumer.panLaw = "unityCenterStereo";
    consumer.externalAudioSourceTrackId = "source";
    consumer.instrument = InternalInstrumentSpec{
        "instrument:a", false,
        instrumentDirectory.getChildFile("definition.json").loadFileAsString(),
        instrumentDirectory.getFullPathName()};
    consumer.midiClips = source.midiClips;
    auto second = consumer;
    second.id = "consumer-b";
    std::get<InternalInstrumentSpec>(*second.instrument).id = "instrument:b";
    snapshot.graph.tracks = {consumer, second, source};
    const auto render = [&](const ExecutionGraph& graph, const juce::String& name) {
        const auto output = directory.get().getChildFile(name + ".wav");
        OfflineRenderer::Result result;
        juce::String error;
        EXPECT_TRUE(
            renderTestSnapshot(graph, formats, output, 0, 960, 48'000, 256, false, result, error))
            << error;
        auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(output));
        juce::AudioBuffer<float> samples(2, 24'000);
        samples.clear();
        if (reader != nullptr) reader->read(&samples, 0, samples.getNumSamples(), 0, true, true);
        return samples;
    };

    // Act: render the same dependency with one and two audible Consumers.
    auto singleGraph = snapshot.graph;
    singleGraph.tracks[1].muted = true;
    const auto single = render(singleGraph, "single");
    const auto parallel = render(snapshot.graph, "parallel");
    auto silentSourceGraph = singleGraph;
    silentSourceGraph.tracks[2].midiClips.clear();
    const auto silentSource = render(silentSourceGraph, "silent-source");

    // Assert: muting a Source affects Master only; its signal is shared once.
    EXPECT_GT(single.getMagnitude(0, single.getNumSamples()), 0.001f);
    float inputDifference = 0.0f;
    for (int channel = 0; channel < 2; ++channel)
        for (int sample = 0; sample < single.getNumSamples(); ++sample) {
            EXPECT_NEAR(parallel.getSample(channel, sample),
                        2.0f * single.getSample(channel, sample), 1.0e-6f);
            inputDifference =
                std::max(inputDifference, std::abs(single.getSample(channel, sample) -
                                                   silentSource.getSample(channel, sample)));
        }
    EXPECT_GT(inputDifference, 0.001f);
    snapshot.graph.tracks[0].externalAudioSourceTrackId = "consumer-b";
    snapshot.graph.tracks[1].externalAudioSourceTrackId = "consumer-a";
    TimelineEngine engine(true);
    juce::String error;
    EXPECT_FALSE(loadTestSnapshot(engine, snapshot, formats, 48'000, 256, error));
    EXPECT_TRUE(error.containsIgnoreCase("cycle")) << error;
}

}  // namespace riffra
