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

    ASSERT_TRUE(engine.setChannelMixControl(
        EffectOwnerSpec{EffectOwnerKindSpec::track, "track:live"}, -6.0f, -1.0f, error))
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
    snapshot.graph.automation.push_back({AutomationTargetKind::trackVolume,
                                         snapshot.graph.tracks.front().id,
                                         {},
                                         AutomationInterpolationSpec::linear,
                                         {{0, -12.0}}});

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

    ASSERT_TRUE(engine.play());
    // Act: render the automation value, then preview a different static gain.
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(input.size()));
    const auto automatedPeak =
        juce::FloatVectorOperations::findMaximum(left.data(), static_cast<int>(left.size()));
    ASSERT_TRUE(engine.setChannelMixControl(
        EffectOwnerSpec{EffectOwnerKindSpec::track, "track:live"}, -24.0f, std::nullopt, error))
        << error.toStdString();
    left.fill(0.0f);
    right.fill(0.0f);
    engine.mix(inputs.data(), 1, outputs.data(), 2, static_cast<int>(input.size()));

    // Assert: the absolute automation value remains authoritative.
    EXPECT_NEAR(
        juce::FloatVectorOperations::findMaximum(left.data(), static_cast<int>(left.size())),
        automatedPeak, 0.01f);

    // Verify volume and pan automation reach their late-block values in one
    // realtime callback, rather than only preserving a static automation point.
    auto rampSnapshot = makeAudioTrackSnapshot(1, true, false);
    rampSnapshot.graph.automation.push_back({AutomationTargetKind::trackVolume,
                                             rampSnapshot.graph.tracks.front().id,
                                             {},
                                             AutomationInterpolationSpec::linear,
                                             {{0, -24.0}, {20, 0.0}}});
    rampSnapshot.graph.automation.push_back({AutomationTargetKind::trackPan,
                                             rampSnapshot.graph.tracks.front().id,
                                             {},
                                             AutomationInterpolationSpec::linear,
                                             {{0, -1.0}, {20, 1.0}}});
    TimelineEngine rampEngine;
    ASSERT_TRUE(loadTestSnapshot(rampEngine, rampSnapshot, formats, 48'000.0, 512, error))
        << error.toStdString();
    ASSERT_TRUE(rampEngine.play());
    std::array<float, 512> rampInput{};
    std::array<float, 512> rampLeft{};
    std::array<float, 512> rampRight{};
    rampInput.fill(0.25f);
    const std::array<const float*, 1> rampInputs{rampInput.data()};
    const std::array<float*, 2> rampOutputs{rampLeft.data(), rampRight.data()};
    rampEngine.mix(rampInputs.data(), 1, rampOutputs.data(), 2, 512);
    EXPECT_GT(rampLeft[20], rampRight[20] * 2.0f);
    EXPECT_GT(rampRight[490], rampLeft[490] * 2.0f);
    EXPECT_GT(rampLeft[490] + rampRight[490], (rampLeft[20] + rampRight[20]) * 4.0f);
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

TEST(TimelineEngineTest, RendersBuiltInAndAutomatedVst3InstrumentsThroughOfflineRenderer) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    for (const bool vst3 : {false, true}) {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        test::TemporaryDirectory directory;
        const auto destination = directory.get().getChildFile("built-in.wav");
        OfflineRenderer::Result result;
        juce::String error;

        // Act
        auto snapshot = makeBuiltInInstrumentSnapshot("track:offline");
        if (vst3) {
            auto& track = snapshot.graph.tracks.front();
            track.panLaw = "unityCenterStereo";
            track.instrument =
                Vst3InstrumentSpec{"instrument:vst", RIFFRA_TEST_INSTRUMENT_BUNDLE_PATH, {}};
            track.midiClips[0].notes[0].durationTicks = 960;
            snapshot.graph.automation = {{AutomationTargetKind::deviceParameter,
                                          "instrument:vst",
                                          "3165055",
                                          AutomationInterpolationSpec::step,
                                          {{0, 0.25}, {240, 0.75}}}};
        }
        const auto rendered = renderTestSnapshot(snapshot.graph, formats, destination, 0, 960,
                                                 48'000, 512, false, result, error);

        // Assert
        ASSERT_TRUE(rendered) << error.toStdString();
        ASSERT_TRUE(destination.existsAsFile());
        ASSERT_GT(result.frames, 0u);
        auto reader =
            std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
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
        if (vst3) {
            EXPECT_NEAR(output.getSample(0, 1000), 0.0625f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 10000), 0.1875f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 6143), 0.0625f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 6144), 0.1875f, 0.0001f);
            auto invalid = snapshot.graph;
            invalid.automation.front().parameterId = "absent";
            EXPECT_TRUE(renderTestSnapshot(invalid, formats,
                                           directory.get().getChildFile("unresolved.wav"), 0, 960,
                                           48'000, 512, false, result, error))
                << error.toStdString();
            ASSERT_EQ(result.automationWarnings.size(), 1u);
            EXPECT_TRUE(result.automationWarnings.front().contains("instrument:vst/absent"));
        }
    }
}

TEST(TimelineEngineTest, OfflineRendererReturnsIdenticalFrameCountWithAndWithoutTerminalNotes) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto terminalSnapshot = makeBuiltInInstrumentSnapshot("track:terminal");
    terminalSnapshot.graph.tracks.front().midiClips.front().notes.front().durationTicks = 960;
    auto plainSnapshot = makeBuiltInInstrumentSnapshot("track:plain");
    OfflineRenderer::Result terminalResult;
    OfflineRenderer::Result plainResult;
    juce::String error;

    // Act
    const auto renderTo = [&](const ExecutionGraph& graph, const char* fileName) {
        const OfflineRenderRequestSpec request{
            graph,  directory.get().getChildFile(fileName).getFullPathName(),
            0,      960,
            48'000, 512,
            false,  0.0,
            true};
        const auto renderer = OfflineRenderer::prepare(request, formats, error);
        OfflineRenderer::Result result;
        const auto success = renderer != nullptr && renderer->render(formats, result, error);
        return std::pair(success, result);
    };
    const auto terminal = renderTo(terminalSnapshot.graph, "terminal.wav");
    const auto plain = renderTo(plainSnapshot.graph, "plain.wav");

    // Assert
    ASSERT_TRUE(terminal.first) << error.toStdString();
    ASSERT_TRUE(plain.first) << error.toStdString();
    EXPECT_EQ(terminal.second.frames, plain.second.frames);
    EXPECT_GT(terminal.second.frames, 0u);
    const auto terminalReader = std::unique_ptr<juce::AudioFormatReader>(
        formats.createReaderFor(directory.get().getChildFile("terminal.wav")));
    ASSERT_NE(terminalReader, nullptr);
    EXPECT_EQ(static_cast<std::uint64_t>(terminalReader->lengthInSamples), terminal.second.frames);
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

TEST(TimelineEngineTest, MixesMonoAndStereoSourcesAtDifferentSampleRates) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto mono = directory.get().getChildFile("mixed-44100-mono.wav");
    const auto stereo = directory.get().getChildFile("mixed-48000-stereo.wav");
    ASSERT_TRUE(writePcmWave(mono, 44'100, 1, 44'100, 6'000));
    ASSERT_TRUE(writePcmWave(stereo, 48'000, 2, 48'000, 4'000));

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.revision = 7;
    auto track = makeAudioTrack("track:mixed");
    track.audioClips = {
        {"mono-44100", mono.getFullPathName(), 44'100, 0, 44'100, 44'100, 44'100, 0, 0, 0,
         FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false},
        {"stereo-48000", stereo.getFullPathName(), 48'000, 0, 48'000, 48'000, 48'000, 0, 0, 0,
         FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false},
    };
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;

    // Act
    ASSERT_TRUE(engine.play());
    std::array<float, 512> left{};
    std::array<float, 512> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    for (int block = 0; block < 8; ++block) engine.mix(outputs.data(), 2, 512);

    // Assert: both sources contribute to the mix once they overlap.
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.1f);
    EXPECT_GT(*std::max_element(right.begin(), right.end()), 0.1f);
}

TEST(TimelineEngineTest, NormalizesAnOfflineRenderToTheTargetPeak) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:render");
    const auto destination = directory.get().getChildFile("normalized.wav");

    // Act
    OfflineRenderer::Result result;
    juce::String error;
    ASSERT_TRUE(renderTestSnapshot(snapshot.graph, formats, destination, 0, 1'920, 48'000, 512,
                                   true, result, error))
        << error;

    // Assert: the rendered output reaches the normalized target peak.
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
    ASSERT_NE(reader, nullptr);
    const auto sampleCount = static_cast<int>(reader->lengthInSamples);
    ASSERT_GT(sampleCount, 0);
    juce::AudioBuffer<float> rendered(2, sampleCount);
    ASSERT_TRUE(reader->read(&rendered, 0, sampleCount, 0, true, true));
    const auto peak = std::max(rendered.getMagnitude(0, 0, sampleCount),
                               rendered.getMagnitude(1, 0, sampleCount));
    EXPECT_NEAR(peak, 0.98f, 0.02f);
}

TEST(MixerRoutingTest, AutomationFollowsMixerRoutesAndRestoresStaticValuesOnStop) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, true, false);
    const auto trackId = snapshot.graph.tracks.front().id;
    snapshot.graph.tracks.front().outputBusId = "group";
    snapshot.graph.tracks.front().panLaw = "unityCenterStereo";
    snapshot.graph.mixer.buses = {
        {"group", MixBusKindSpec::group, 0.0, 0.0, "unityCenterStereo", false, false, {}},
        {"return", MixBusKindSpec::returnBus, 0.0, 0.0, "unityCenterStereo", false, false, {}}};
    snapshot.graph.mixer.sends = {
        {"send", trackId, "return", 0.0, SendPositionSpec::preFader, true}};
    snapshot.graph.automation = {{AutomationTargetKind::trackVolume,
                                  trackId,
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -6.0}, {20, -12.0}}},
                                 {AutomationTargetKind::trackPan,
                                  trackId,
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, 0.0}, {20, 1.0}}},
                                 {AutomationTargetKind::busGain,
                                  "group",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -6.0}, {20, -12.0}}},
                                 {AutomationTargetKind::busPan,
                                  "group",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -1.0}, {20, 0.0}}},
                                 {AutomationTargetKind::busGain,
                                  "return",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -6.0}, {20, -12.0}}},
                                 {AutomationTargetKind::busPan,
                                  "return",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, 1.0}, {20, 0.0}}},
                                 {AutomationTargetKind::sendLevel,
                                  "send",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -6.0}, {20, -12.0}}},
                                 {AutomationTargetKind::masterGain,
                                  {},
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, -6.0}, {20, -12.0}}}};
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 1024, error))
        << error.toStdString();
    std::array<float, 1024> input, left{}, right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_TRUE(engine.play());
    engine.mix(inputs.data(), 1, outputs.data(), 2, 1024);
    engine.applyMasterGain(outputs.data(), 2, 1024, 0.0f);

    // Assert: exact Step boundary at tick 20 (sample 500), after the transport fade.
    const auto first = 0.25f * std::pow(10.0f, -18.0f / 20.0f);
    EXPECT_NEAR(left[499], first, 0.00001f);
    EXPECT_NEAR(right[499], first, 0.00001f);
    const auto late = 0.25f * std::pow(10.0f, -36.0f / 20.0f);
    EXPECT_NEAR(left[500], late, 0.00001f);
    EXPECT_NEAR(right[500], 2.0f * late, 0.00001f);
    ASSERT_TRUE(engine.stop());
    for (int block = 0; block < 2; ++block) {
        left.fill(0.0f);
        right.fill(0.0f);
        engine.mix(inputs.data(), 1, outputs.data(), 2, 1024);
        engine.applyMasterGain(outputs.data(), 2, 1024, 0.0f);
    }
    EXPECT_NEAR(left.back(), 0.5f, 0.00001f);
    EXPECT_NEAR(right.back(), 0.5f, 0.00001f);
    auto looped = snapshot;
    looped.graph.loopRange = {true, 0, 2};
    looped.graph.automation.back().points = {{0, -6.0}, {1, -12.0}};
    TimelineEngine loopEngine;
    ASSERT_TRUE(loadTestSnapshot(loopEngine, looped, formats, 48'000.0, 4096, error)) << error;
    ASSERT_TRUE(loopEngine.play());
    std::array<float, 4096> loopInput, loopLeft{}, loopRight{};
    loopInput.fill(0.25f);
    const std::array<const float*, 1> loopInputs{loopInput.data()};
    const std::array<float*, 2> loopOutputs{loopLeft.data(), loopRight.data()};
    loopEngine.mix(loopInputs.data(), 1, loopOutputs.data(), 2, 4096);
    loopEngine.applyMasterGain(loopOutputs.data(), 2, 4096, 0.0f);
    EXPECT_NEAR(loopLeft[4000], first, 0.00001f);
    EXPECT_NEAR(loopLeft[4025], first * std::pow(10.0f, -6.0f / 20.0f), 0.00001f);
}

TEST(MixerRoutingTest, AlignsRealVstPathsAndReusesDevicesOnlyWhenProjectedStateMatches) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("mixer.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    AudioClipSpec clip;
    clip.id = "clip:source";
    clip.path = source.getFullPathName();
    clip.startTick = 20;
    clip.sourceSampleRate = 48'000;
    clip.sourceEndFrame = 128;
    clip.durationFrames = 128;
    clip.durationSampleRate = 48'000;
    auto first = makeAudioTrack("track:first");
    first.panLaw = "unityCenterStereo";
    first.outputBusId = "bus:group";
    first.audioClips.push_back(clip);
    first.effects.push_back({"fx:first", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}});
    auto second = makeAudioTrack("track:second");
    second.panLaw = "unityCenterStereo";
    clip.id = "clip:second";
    second.audioClips.push_back(clip);
    second.effects = {{"fx:second:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                      {"fx:second:2", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    snapshot.graph.tracks = {first, second};
    MixBusSpec group;
    group.id = "bus:group";
    group.effects = {{"fx:group", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    auto returnBus = group;
    returnBus.id = "bus:return";
    returnBus.kind = MixBusKindSpec::returnBus;
    returnBus.effects.front().id = "fx:return";
    snapshot.graph.mixer.buses = {group, returnBus};
    snapshot.graph.mixer.sends = {
        {"send:first", first.id, returnBus.id, 0.0, SendPositionSpec::preFader, true}};
    snapshot.graph.mixer.masterEffects = {{"fx:master", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};

    for (const bool masterBypassed : {false, true}) {
        auto candidate = snapshot;
        candidate.graph.mixer.masterEffects.front().state.bypassed = masterBypassed;
        TimelineEngine engine(true);
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        auto* graph = TimelineEngineTestPeer::committedGraph(engine);
        ASSERT_NE(graph, nullptr);
        const auto latency = masterBypassed ? 16 : 24;
        EXPECT_EQ(graph->summary.maximumLatencySamples, latency);
        ASSERT_TRUE(engine.play());
        juce::AudioBuffer<float> output(2, 704);
        output.clear();

        // Act
        engine.mix(output.getArrayOfWritePointers(), 2, 704);

        // Assert: all dry and parallel paths arrive together, once.
        const auto signal = 0.25f / std::sqrt(2.0f);
        const auto expected = signal * (masterBypassed ? 0.75f : 0.375f);
        EXPECT_NEAR(output.getSample(0, 500 + latency - 1), 0.0f, 0.0001f);
        EXPECT_NEAR(output.getSample(0, 500 + latency), expected, 0.002f);
        EXPECT_NEAR(output.getSample(1, 500 + latency + 32), expected, 0.002f);
        const auto meters = engine.busMeterSnapshot();
        ASSERT_EQ(meters.size(), 2u);
        EXPECT_GT(meters[0].peakLeft, 0.01);
        const auto groupEffects = graph->mixer.buses[0]->effects;
        const auto masterEffects = graph->mixer.masterEffects;
        const auto automatedOwner = EffectOwnerSpec{EffectOwnerKindSpec::bus, "bus:group"};
        const auto staticState = engine.devicePersistedState(automatedOwner, "fx:group", error);
        ASSERT_TRUE(staticState.has_value()) << error;
        const auto parameterInfo = groupEffects->findDevice("fx:group")->parameters();
        ASSERT_TRUE(parameterInfo[0].parameterId.has_value());
        candidate.graph.automation = {{AutomationTargetKind::deviceParameter,
                                       "fx:group",
                                       *parameterInfo[0].parameterId,
                                       AutomationInterpolationSpec::step,
                                       {{0, 0.25}}}};
        candidate.graph.mixer.buses[0].gainDb = -3.0;
        candidate.graph.mixer.sends[0].levelDb = -6.0;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        graph = TimelineEngineTestPeer::committedGraph(engine);
        EXPECT_EQ(graph->mixer.buses[0]->effects, groupEffects);
        EXPECT_EQ(graph->mixer.masterEffects, masterEffects);
        ASSERT_EQ(graph->mixer.buses[0]->deviceAutomation.size(), 1u);
        output.clear();
        engine.mix(output.getArrayOfWritePointers(), 2, 64);
        EXPECT_EQ(engine.devicePersistedState(automatedOwner, "fx:group", error), staticState);
        EXPECT_FALSE(engine.setDeviceParameter(automatedOwner, "fx:group", 0, 0.9f, error));
        candidate.graph.automation.clear();
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        EXPECT_EQ(TimelineEngineTestPeer::committedGraph(engine)->mixer.buses[0]->effects,
                  groupEffects);
        EXPECT_NEAR(groupEffects->findDevice("fx:group")->parameters()[0].value,
                    staticState->parameterValues[0], 0.0001f);
        for (const auto& [owner, id] : std::array<std::pair<EffectOwnerSpec, juce::String>, 3>{
                 std::pair{EffectOwnerSpec{EffectOwnerKindSpec::bus, "bus:group"},
                           juce::String("fx:group")},
                 std::pair{EffectOwnerSpec{EffectOwnerKindSpec::bus, "bus:return"},
                           juce::String("fx:return")},
                 std::pair{EffectOwnerSpec{EffectOwnerKindSpec::master, {}},
                           juce::String("fx:master")}}) {
            const auto originalState = engine.devicePersistedState(owner, id, error);
            ASSERT_TRUE(originalState.has_value()) << error;
            ASSERT_TRUE(engine.setDeviceParameter(owner, id, 0, 0.75f, error)) << error;
            auto state = engine.devicePersistedState(owner, id, error);
            ASSERT_TRUE(state.has_value()) << error;
            ASSERT_EQ(state->parameterValues.size(), originalState->parameterValues.size());
            for (std::size_t index = 1; index < state->parameterValues.size(); ++index)
                EXPECT_NEAR(state->parameterValues[index], originalState->parameterValues[index],
                            0.001f);
            EXPECT_NEAR(state->parameterValues[0], 0.75f, 0.001f);
            ASSERT_TRUE(state->stateData.has_value());
            ASSERT_TRUE(engine.setDeviceParameter(owner, id, 0, 0.25f, error));
            ASSERT_TRUE(engine.setDevicePersistedState(owner, id, *state, error)) << error;
            EXPECT_NEAR(engine.devicePersistedState(owner, id, error)->parameterValues[0], 0.75f,
                        0.001f);
            ASSERT_TRUE(engine.setDeviceBypassed(owner, id, true, error));
            EXPECT_TRUE(engine.devicePersistedState(owner, id, error)->bypassed);
        }
        for (auto& busSpec : candidate.graph.mixer.buses) {
            for (auto& effect : busSpec.effects)
                effect.state = *engine.devicePersistedState(
                    EffectOwnerSpec{EffectOwnerKindSpec::bus, busSpec.id}, effect.id, error);
        }
        for (auto& effect : candidate.graph.mixer.masterEffects)
            effect.state = *engine.devicePersistedState(
                EffectOwnerSpec{EffectOwnerKindSpec::master, {}}, effect.id, error);
        candidate.graph.mixer.buses[0].gainDb = -9.0;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        graph = TimelineEngineTestPeer::committedGraph(engine);
        EXPECT_NE(graph->mixer.buses[0]->effects, groupEffects);
        EXPECT_NE(graph->mixer.masterEffects, masterEffects);
        ASSERT_TRUE(engine.setChannelMixControl(
            EffectOwnerSpec{EffectOwnerKindSpec::bus, "bus:group"}, -9.0f, 0.2f, error));
        ASSERT_TRUE(engine.setSendLevelControl("send:first", -12.0f, error));
    }
}

TEST(MixerRoutingTest, AppliesMuteAndAdditiveSoloToDryAndParallelPaths) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("solo.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    auto first = makeAudioTrack("track:first");
    first.panLaw = "unityCenterStereo";
    first.outputBusId = "bus:group";
    AudioClipSpec clip;
    clip.id = "clip:first";
    clip.path = source.getFullPathName();
    clip.startTick = 20;
    clip.sourceSampleRate = 48'000;
    clip.sourceEndFrame = 128;
    clip.durationFrames = 128;
    clip.durationSampleRate = 48'000;
    first.audioClips.push_back(clip);
    auto second = first;
    second.id = "track:second";
    second.outputBusId.reset();
    second.gainDb = -6.020599913;
    second.audioClips.front().id = "clip:second";
    snapshot.graph.tracks = {first, second};
    MixBusSpec group;
    group.id = "bus:group";
    auto returnBus = group;
    returnBus.id = "bus:return";
    returnBus.kind = MixBusKindSpec::returnBus;
    snapshot.graph.mixer.buses = {group, returnBus};
    snapshot.graph.mixer.sends = {
        {"send:first", first.id, returnBus.id, 0.0, SendPositionSpec::postFader, true},
        {"send:second", second.id, returnBus.id, 0.0, SendPositionSpec::postFader, true}};
    struct Case {
        bool trackSolo, groupSolo, returnSolo, trackMute, groupMute, returnMute;
        float expected;
    };
    const std::array<Case, 8> cases{{
        {false, false, false, false, false, false, 3.0f},
        {true, false, false, false, false, false, 2.0f},
        {false, true, false, false, false, false, 1.0f},
        {false, false, true, false, false, false, 1.5f},
        {true, false, true, false, false, false, 2.5f},
        {false, false, false, true, false, false, 1.0f},
        {true, false, false, false, true, false, 1.0f},
        {false, false, false, false, false, true, 1.5f},
    }};
    for (const auto& testCase : cases) {
        auto candidate = snapshot;
        candidate.graph.tracks[0].solo = testCase.trackSolo;
        candidate.graph.tracks[0].muted = testCase.trackMute;
        candidate.graph.mixer.buses[0].solo = testCase.groupSolo;
        candidate.graph.mixer.buses[0].muted = testCase.groupMute;
        candidate.graph.mixer.buses[1].solo = testCase.returnSolo;
        candidate.graph.mixer.buses[1].muted = testCase.returnMute;
        TimelineEngine engine(true);
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        ASSERT_TRUE(engine.play());
        juce::AudioBuffer<float> output(2, 704);
        output.clear();

        // Act
        engine.mix(output.getArrayOfWritePointers(), 2, 704);

        // Assert
        EXPECT_NEAR(output.getSample(0, 532), 0.25f / std::sqrt(2.0f) * testCase.expected, 0.002f);
    }
    for (const auto position : {SendPositionSpec::preFader, SendPositionSpec::postFader}) {
        auto candidate = snapshot;
        candidate.graph.tracks[1].muted = true;
        candidate.graph.tracks[0].gainDb = -6.020599913;
        candidate.graph.mixer.sends[0].position = position;
        candidate.graph.mixer.buses[1].solo = true;
        TimelineEngine engine(true);
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        ASSERT_TRUE(engine.play());
        juce::AudioBuffer<float> output(2, 704);
        output.clear();
        engine.mix(output.getArrayOfWritePointers(), 2, 704);
        const auto expected = position == SendPositionSpec::preFader ? 1.0f : 0.5f;
        EXPECT_NEAR(output.getSample(0, 532), 0.25f / std::sqrt(2.0f) * expected, 0.002f);
    }
}

TEST(MixerRoutingTest, SendsSidechainAudioToSupportedTargetsAndKeepsUnsupportedTargetsOnline) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("sidechain.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();

    enum class Target { track, group, master, unsupported };
    for (const auto target : {Target::track, Target::group, Target::master, Target::unsupported}) {
        SCOPED_TRACE(static_cast<int>(target));
        const auto build = [&](const bool withSend) {
            auto snapshot = makeTestSnapshot();
            auto audio = makeAudioTrack("track:source");
            audio.panLaw = "unityCenterStereo";
            AudioClipSpec clip;
            clip.id = "clip:source";
            clip.path = source.getFullPathName();
            clip.startTick = 20;
            clip.sourceSampleRate = 48'000;
            clip.sourceEndFrame = 128;
            clip.durationFrames = 128;
            clip.durationSampleRate = 48'000;
            audio.audioClips.push_back(clip);
            if (target == Target::group) audio.outputBusId = "bus:group";
            snapshot.graph.tracks.push_back(std::move(audio));
            juce::String deviceId;
            if (target == Target::track || target == Target::unsupported) {
                auto owner = makeAudioTrack("track:target");
                owner.panLaw = "unityCenterStereo";
                owner.effects.push_back({"fx:target",
                                         target == Target::track ? RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH
                                                                 : RIFFRA_TEST_EFFECT_BUNDLE_PATH,
                                         {}});
                deviceId = "fx:target";
                snapshot.graph.tracks.push_back(std::move(owner));
            } else if (target == Target::group) {
                MixBusSpec group;
                group.id = "bus:group";
                group.panLaw = "unityCenterStereo";
                group.effects.push_back({"fx:group", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
                deviceId = "fx:group";
                snapshot.graph.mixer.buses.push_back(std::move(group));
            } else {
                deviceId = "fx:master";
                snapshot.graph.mixer.masterEffects.push_back(
                    {deviceId, RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
            }
            if (withSend)
                snapshot.graph.mixer.sidechainSends.push_back({"send:sidechain", "track:source",
                                                               deviceId, 0.0,
                                                               SendPositionSpec::postFader, true});
            return snapshot;
        };

        std::array<juce::AudioBuffer<float>, 2> outputs{juce::AudioBuffer<float>(2, 704),
                                                        juce::AudioBuffer<float>(2, 704)};
        for (int variant = 0; variant < 2; ++variant) {
            SCOPED_TRACE(variant == 0 ? "without sidechain" : "with sidechain");
            TimelineEngine engine(true);
            juce::String error;
            {
                SCOPED_TRACE("load snapshot");
                ASSERT_TRUE(
                    loadTestSnapshot(engine, build(variant == 1), formats, 48'000.0, 64, error))
                    << error;
                if (target == Target::unsupported && variant == 1) {
                    const auto* graph = TimelineEngineTestPeer::committedGraph(engine);
                    ASSERT_NE(graph, nullptr);
                    EXPECT_TRUE(graph->mixer.sidechainRoutes.empty());
                    const auto* targetTrack = graph->findTrack("track:target");
                    ASSERT_NE(targetTrack, nullptr);
                    EXPECT_EQ(targetTrack->runtime->effects().sidechainBuffer("fx:target"),
                              nullptr);
                }
            }
            {
                SCOPED_TRACE("start playback");
                ASSERT_TRUE(engine.play());
            }
            {
                SCOPED_TRACE("mix output");
                auto& rendered = outputs[static_cast<std::size_t>(variant)];
                rendered.clear();
                engine.mix(rendered.getArrayOfWritePointers(), 2, 704);
            }
        }

        // Assert
        auto doubledPeak = 0.0f;
        for (int sample = 0; sample < 704; ++sample) {
            for (int channel = 0; channel < 2; ++channel) {
                const auto baseline = outputs[0].getSample(channel, sample);
                const auto rendered = outputs[1].getSample(channel, sample);
                if (target == Target::unsupported) {
                    EXPECT_NEAR(rendered, baseline, 0.0001f) << "sample " << sample;
                } else {
                    const auto expected = 2.0f * baseline;
                    EXPECT_NEAR(rendered, expected, 0.0001f) << "sample " << sample;
                    doubledPeak = std::max(doubledPeak, std::abs(expected));
                }
            }
        }
        if (target != Target::unsupported) EXPECT_GT(doubledPeak, 0.3f);
    }
}

TEST(MixerRoutingTest, AlignsMultipleSidechainSourcesWithTheTargetDeviceLatency) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto loud = directory.get().getChildFile("loud.wav");
    const auto quiet = directory.get().getChildFile("quiet.wav");
    ASSERT_TRUE(writePcmWave(loud, 48'000, 1, 128, 8'192));
    ASSERT_TRUE(writePcmWave(quiet, 48'000, 1, 128, 4'096));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    const auto makeClip = [](const juce::String& id, const juce::File& file) {
        AudioClipSpec clip;
        clip.id = id;
        clip.path = file.getFullPathName();
        clip.startTick = 20;
        clip.sourceSampleRate = 48'000;
        clip.sourceEndFrame = 128;
        clip.durationFrames = 128;
        clip.durationSampleRate = 48'000;
        return clip;
    };
    const auto build = [&](const bool withSend) {
        auto snapshot = makeTestSnapshot();
        auto latent = makeAudioTrack("track:latent");
        latent.panLaw = "unityCenterStereo";
        latent.audioClips.push_back(makeClip("clip:latent", loud));
        latent.effects.push_back({"fx:latent", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}});
        auto dry = makeAudioTrack("track:dry");
        dry.panLaw = "unityCenterStereo";
        dry.audioClips.push_back(makeClip("clip:dry", quiet));
        auto target = makeAudioTrack("track:target");
        target.panLaw = "unityCenterStereo";
        target.effects.push_back({"fx:target", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
        snapshot.graph.tracks = {latent, dry, target};
        if (withSend) {
            snapshot.graph.mixer.sidechainSends.push_back({"send:latent", "track:latent",
                                                           "fx:target", 0.0,
                                                           SendPositionSpec::postFader, true});
            snapshot.graph.mixer.sidechainSends.push_back(
                {"send:dry", "track:dry", "fx:target", 0.0, SendPositionSpec::postFader, true});
        }
        return snapshot;
    };

    std::array<juce::AudioBuffer<float>, 2> outputs{juce::AudioBuffer<float>(2, 704),
                                                    juce::AudioBuffer<float>(2, 704)};
    for (int variant = 0; variant < 2; ++variant) {
        TimelineEngine engine(true);
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, build(variant == 1), formats, 48'000.0, 64, error))
            << error;
        ASSERT_TRUE(engine.play());
        auto& rendered = outputs[static_cast<std::size_t>(variant)];
        rendered.clear();
        engine.mix(rendered.getArrayOfWritePointers(), 2, 704);
        if (variant == 1) {
            // A transport discontinuity must clear the auxiliary buffers and the
            // device main input delays so the replay is identical.
            ASSERT_TRUE(engine.stop());
            ASSERT_TRUE(engine.seekToTick(0));
            ASSERT_TRUE(engine.play());
            juce::AudioBuffer<float> replay(2, 704);
            replay.clear();
            engine.mix(replay.getArrayOfWritePointers(), 2, 704);
            for (int sample = 0; sample < 704; ++sample)
                for (int channel = 0; channel < 2; ++channel)
                    EXPECT_NEAR(replay.getSample(channel, sample),
                                rendered.getSample(channel, sample), 0.0001f)
                        << "sample " << sample;
        }
    }

    // Assert: both sources keep their own latency-compensated timing and the
    // auxiliary copies double the reference pulse exactly.
    auto doubledPeak = 0.0f;
    for (int sample = 0; sample < 704; ++sample) {
        for (int channel = 0; channel < 2; ++channel) {
            const auto expected = 2.0f * outputs[0].getSample(channel, sample);
            EXPECT_NEAR(outputs[1].getSample(channel, sample), expected, 0.0001f)
                << "sample " << sample;
            doubledPeak = std::max(doubledPeak, std::abs(expected));
        }
    }
    EXPECT_GT(doubledPeak, 0.3f);
}

TEST(MixerRoutingTest, AppliesSidechainSendLevelPositionMuteAndSolo) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("levels.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    const auto kContent = 0.25f / std::sqrt(2.0f);

    struct Case {
        const char* name;
        bool sendEnabled;
        bool muted;
        bool targetSolo;
        double levelDb;
        SendPositionSpec position;
        double gainDb;
        float expectedAtPulse;
    };
    const std::array<Case, 6> cases{{
        {"post fader unity", true, false, false, 0.0, SendPositionSpec::postFader, 0.0,
         2.0f * kContent},
        {"pre fader skips the fader", true, false, false, 0.0, SendPositionSpec::preFader,
         -6.020599913, 1.5f * kContent},
        {"send level scales the tap", true, false, false, -6.020599913, SendPositionSpec::postFader,
         -6.020599913, 0.75f * kContent},
        {"disabled send is silent", false, false, false, 0.0, SendPositionSpec::postFader, 0.0,
         kContent},
        {"muted source is silent", true, true, false, 0.0, SendPositionSpec::postFader, 0.0, 0.0f},
        {"solo keeps the auxiliary path", true, false, true, 0.0, SendPositionSpec::postFader, 0.0,
         kContent},
    }};
    for (const auto& testCase : cases) {
        SCOPED_TRACE(testCase.name);
        auto snapshot = makeTestSnapshot();
        auto audio = makeAudioTrack("track:source");
        audio.panLaw = "unityCenterStereo";
        audio.gainDb = testCase.gainDb;
        audio.muted = testCase.muted;
        AudioClipSpec clip;
        clip.id = "clip:source";
        clip.path = source.getFullPathName();
        clip.startTick = 20;
        clip.sourceSampleRate = 48'000;
        clip.sourceEndFrame = 128;
        clip.durationFrames = 128;
        clip.durationSampleRate = 48'000;
        audio.audioClips.push_back(clip);
        auto target = makeAudioTrack("track:target");
        target.panLaw = "unityCenterStereo";
        target.solo = testCase.targetSolo;
        target.effects.push_back({"fx:target", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
        snapshot.graph.tracks = {audio, target};
        if (testCase.sendEnabled)
            snapshot.graph.mixer.sidechainSends.push_back({"send:sidechain", "track:source",
                                                           "fx:target", testCase.levelDb,
                                                           testCase.position, true});
        TimelineEngine engine(true);
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
        ASSERT_TRUE(engine.play());
        juce::AudioBuffer<float> output(2, 704);
        output.clear();

        // Act
        engine.mix(output.getArrayOfWritePointers(), 2, 704);

        // Assert
        EXPECT_NEAR(output.getSample(0, 560), testCase.expectedAtPulse, 0.002f);
        EXPECT_NEAR(output.getSample(1, 560), testCase.expectedAtPulse, 0.002f);
        EXPECT_NEAR(output.getSample(0, 499), 0.0f, 0.0001f);
        EXPECT_NEAR(output.getSample(0, 628), 0.0f, 0.0001f);
    }
}

TEST(MixerRoutingTest, SidechainSendLevelFollowsAutomationInRealtimeAndOffline) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("automated.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    const auto kContent = 0.25f / std::sqrt(2.0f);
    auto snapshot = makeTestSnapshot();
    auto audio = makeAudioTrack("track:source");
    audio.panLaw = "unityCenterStereo";
    AudioClipSpec clip;
    clip.id = "clip:source";
    clip.path = source.getFullPathName();
    clip.startTick = 20;
    clip.sourceSampleRate = 48'000;
    clip.sourceEndFrame = 128;
    clip.durationFrames = 128;
    clip.durationSampleRate = 48'000;
    audio.audioClips.push_back(clip);
    auto target = makeAudioTrack("track:target");
    target.panLaw = "unityCenterStereo";
    target.effects.push_back({"fx:target", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
    snapshot.graph.tracks = {audio, target};
    snapshot.graph.mixer.sidechainSends.push_back(
        {"send:sidechain", "track:source", "fx:target", 0.0, SendPositionSpec::postFader, true});
    snapshot.graph.automation = {{AutomationTargetKind::sidechainSendLevel,
                                  "send:sidechain",
                                  {},
                                  AutomationInterpolationSpec::step,
                                  {{0, 0.0}, {24, -6.020599913}}}};
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    ASSERT_TRUE(engine.play());
    juce::AudioBuffer<float> output(2, 704);
    output.clear();

    // Act: the clip pulse spans tick 20 (sample 500) past the step boundary at
    // tick 24 (sample 600), which halves the automated auxiliary gain.
    engine.mix(output.getArrayOfWritePointers(), 2, 704);

    // Assert
    const auto unity = 2.0f * kContent;
    const auto halved = 1.5f * kContent;
    EXPECT_NEAR(output.getSample(0, 599), unity, 0.002f);
    EXPECT_NEAR(output.getSample(0, 600), halved, 0.002f);
    EXPECT_NEAR(output.getSample(1, 599), unity, 0.002f);
    EXPECT_NEAR(output.getSample(1, 600), halved, 0.002f);

    // Assert: the offline renderer evaluates the same lane on the same samples.
    OfflineRenderer::Result result;
    const auto destination = directory.get().getChildFile("automated-render.wav");
    ASSERT_TRUE(renderTestSnapshot(snapshot.graph, formats, destination, 0, 48, 48'000, 64, false,
                                   result, error))
        << error;
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
    ASSERT_NE(reader, nullptr);
    ASSERT_GT(reader->lengthInSamples, 600);
    juce::AudioBuffer<float> rendered(2, 601);
    ASSERT_TRUE(reader->read(&rendered, 0, rendered.getNumSamples(), 0, true, true));
    EXPECT_NEAR(rendered.getSample(0, 599), unity, 0.002f);
    EXPECT_NEAR(rendered.getSample(0, 600), halved, 0.002f);
    EXPECT_NEAR(rendered.getSample(0, 599), output.getSample(0, 599), 0.00001f);
    EXPECT_NEAR(rendered.getSample(0, 600), output.getSample(0, 600), 0.00001f);

    // A stem isolates the target's main output by solo while preserving the
    // auxiliary source and its automation. The source's dry signal is absent.
    snapshot.graph.tracks[1].solo = true;
    const auto stemDestination = directory.get().getChildFile("automated-stem.wav");
    ASSERT_TRUE(renderTestSnapshot(snapshot.graph, formats, stemDestination, 0, 48, 48'000, 64,
                                   false, result, error))
        << error;
    auto stemReader =
        std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(stemDestination));
    ASSERT_NE(stemReader, nullptr);
    juce::AudioBuffer<float> stem(2, 601);
    ASSERT_TRUE(stemReader->read(&stem, 0, stem.getNumSamples(), 0, true, true));
    for (int channel = 0; channel < 2; ++channel) {
        EXPECT_NEAR(stem.getSample(channel, 599), kContent, 0.002f);
        EXPECT_NEAR(stem.getSample(channel, 600), 0.5f * kContent, 0.002f);
    }
}

TEST(MixerRoutingTest, OverridesSidechainSendLevelControlOnThePreparedRoute) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("preview.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    const auto kContent = 0.25f / std::sqrt(2.0f);
    auto snapshot = makeTestSnapshot();
    auto audio = makeAudioTrack("track:source");
    audio.panLaw = "unityCenterStereo";
    AudioClipSpec clip;
    clip.id = "clip:source";
    clip.path = source.getFullPathName();
    clip.startTick = 20;
    clip.sourceSampleRate = 48'000;
    clip.sourceEndFrame = 128;
    clip.durationFrames = 128;
    clip.durationSampleRate = 48'000;
    audio.audioClips.push_back(clip);
    auto target = makeAudioTrack("track:target");
    target.panLaw = "unityCenterStereo";
    target.effects.push_back({"fx:target", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
    snapshot.graph.tracks = {audio, target};
    snapshot.graph.mixer.sidechainSends.push_back(
        {"send:sidechain", "track:source", "fx:target", 0.0, SendPositionSpec::postFader, true});
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    ASSERT_TRUE(engine.play());
    juce::AudioBuffer<float> output(2, 704);
    output.clear();

    // Act: the transient level halves the sidechain route gain.
    ASSERT_TRUE(engine.setSendLevelControl("send:sidechain", -6.020599913f, error)) << error;
    engine.mix(output.getArrayOfWritePointers(), 2, 704);

    // Assert
    EXPECT_NEAR(output.getSample(0, 599), 1.5f * kContent, 0.002f);
    EXPECT_FALSE(engine.setSendLevelControl("send:missing", 0.0f, error));
    EXPECT_TRUE(error.containsIgnoreCase("unavailable")) << error;
}

TEST(MixerRoutingTest, RejectsCyclicSidechainRoutes) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    auto first = makeAudioTrack("track:first");
    first.effects.push_back({"fx:first", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
    auto second = makeAudioTrack("track:second");
    second.effects.push_back({"fx:second", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
    snapshot.graph.tracks = {first, second};
    snapshot.graph.mixer.sidechainSends = {
        {"send:first", "track:first", "fx:second", 0.0, SendPositionSpec::postFader, true},
        {"send:second", "track:second", "fx:first", 0.0, SendPositionSpec::postFader, true}};
    TimelineEngine engine;
    juce::String error;

    // Act
    const auto loaded = loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error);

    // Assert
    EXPECT_FALSE(loaded);
    EXPECT_TRUE(error.containsIgnoreCase("cycle")) << error;
}

TEST(MixerRoutingTest, LowLatencyMonitoringKeepsTimelinePdcAcrossMasterAndGroupRoutes) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("mixed-low-latency.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    const auto makeClip = [&source](const juce::String& id) {
        AudioClipSpec clip;
        clip.id = id;
        clip.path = source.getFullPathName();
        clip.startTick = 20;
        clip.sourceSampleRate = 48'000;
        clip.sourceEndFrame = 128;
        clip.durationFrames = 128;
        clip.durationSampleRate = 48'000;
        return clip;
    };
    auto monitoredTrack = makeAudioTrack("track:monitored");
    monitoredTrack.monitorInput = true;
    monitoredTrack.audioInput = AudioInputSpec{0};
    monitoredTrack.outputBusId = "bus:group";
    monitoredTrack.panLaw = "unityCenterStereo";
    monitoredTrack.audioClips.push_back(makeClip("clip:monitored"));
    auto processedClip = makeClip("clip:processed");
    processedClip.takeVariant = TakeVariantSpec::processed;
    monitoredTrack.audioClips.push_back(processedClip);
    auto highLatencyTrack = makeAudioTrack("track:high-latency");
    highLatencyTrack.panLaw = "unityCenterStereo";
    highLatencyTrack.audioClips.push_back(makeClip("clip:high-latency"));
    highLatencyTrack.effects = {{"fx:high:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                                {"fx:high:2", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                                {"fx:high:3", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    snapshot.graph.tracks = {monitoredTrack, highLatencyTrack};
    MixBusSpec group;
    group.id = "bus:group";
    group.effects = {{"fx:group", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    snapshot.graph.mixer.buses.push_back(group);

    for (const auto throughGroup : {true, false}) {
        auto candidate = snapshot;
        if (!throughGroup) candidate.graph.tracks[0].outputBusId.reset();
        TimelineEngine engine;
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, candidate, formats, 48'000.0, 64, error)) << error;
        const auto* graph = TimelineEngineTestPeer::committedGraph(engine);
        ASSERT_NE(graph, nullptr);
        EXPECT_EQ(graph->summary.maximumLatencySamples, 24u);
        ASSERT_TRUE(engine.seekToTick(0));
        ASSERT_TRUE(engine.play());
        std::array<float, 704> input{};
        std::fill_n(input.begin(), 64, 0.5f);
        const std::array<const float*, 1> inputs{input.data()};
        juce::AudioBuffer<float> output(2, static_cast<int>(input.size()));
        output.clear();

        // Act: one monitored Track carries live input and a Timeline clip.
        engine.mix(inputs.data(), 1, output.getArrayOfWritePointers(), 2,
                   static_cast<int>(input.size()));

        // Assert: live input bypasses inter-route PDC while the clip remains aligned.
        auto livePeak = 0.0f;
        auto livePeakSample = -1;
        auto firstLiveSample = -1;
        for (int sample = 0; sample < 128; ++sample) {
            const auto level = std::abs(output.getSample(0, sample));
            if (firstLiveSample < 0 && level > 0.001f) firstLiveSample = sample;
            if (level > livePeak) {
                livePeak = level;
                livePeakSample = sample;
            }
        }
        EXPECT_GT(livePeak, 0.01f)
            << "peak sample " << livePeakSample << ", throughGroup " << throughGroup;
        EXPECT_EQ(firstLiveSample, throughGroup ? 9 : 1)
            << "peak sample " << livePeakSample << ", throughGroup " << throughGroup;
        EXPECT_NEAR(output.getSample(0, 500 + 8), 0.0f, 0.0001f);
        EXPECT_GT(output.getSample(0, 500 + 24), 0.01f);
    }
}

TEST(MixerRoutingTest, DisablesLowLatencyPdcBypassForDependentOrAutomatedTracks) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("restricted-low-latency.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();

    enum class Restriction { externalAudioSource, volumeAutomation, panAutomation };
    for (const auto restriction : {Restriction::externalAudioSource, Restriction::volumeAutomation,
                                   Restriction::panAutomation}) {
        SCOPED_TRACE(static_cast<int>(restriction));
        auto snapshot = makeTestSnapshot();
        auto monitoredTrack = makeAudioTrack("track:monitored");
        monitoredTrack.monitorInput = true;
        monitoredTrack.audioInput = AudioInputSpec{0};
        AudioClipSpec clip;
        clip.id = "clip:monitored";
        clip.path = source.getFullPathName();
        clip.sourceSampleRate = 48'000;
        clip.sourceEndFrame = 128;
        clip.durationFrames = 128;
        clip.durationSampleRate = 48'000;
        monitoredTrack.audioClips.push_back(clip);
        if (restriction == Restriction::volumeAutomation)
            snapshot.graph.automation.push_back({AutomationTargetKind::trackVolume,
                                                 monitoredTrack.id,
                                                 {},
                                                 AutomationInterpolationSpec::linear,
                                                 {{0, -6.0}}});
        if (restriction == Restriction::panAutomation)
            snapshot.graph.automation.push_back({AutomationTargetKind::trackPan,
                                                 monitoredTrack.id,
                                                 {},
                                                 AutomationInterpolationSpec::linear,
                                                 {{0, 0.5}}});

        auto highLatencyTrack = makeAudioTrack("track:high-latency");
        highLatencyTrack.audioClips.push_back(clip);
        highLatencyTrack.audioClips.back().id = "clip:high-latency";
        highLatencyTrack.effects = {{"fx:high:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                                    {"fx:high:2", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                                    {"fx:high:3", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
        snapshot.graph.tracks = {monitoredTrack};
        if (restriction == Restriction::externalAudioSource) {
            auto consumer = makeInstrumentTrack("track:consumer");
            consumer.externalAudioSourceTrackId = monitoredTrack.id;
            const auto instrumentDirectory = juce::File(RIFFRA_CONTRACT_FIXTURE_DIR)
                                                 .getSiblingFile("sonalloy-bundle")
                                                 .getChildFile("basic/instruments/duck");
            consumer.instrument = InternalInstrumentSpec{
                "instrument:consumer", false,
                instrumentDirectory.getChildFile("definition.json").loadFileAsString(),
                instrumentDirectory.getFullPathName()};
            snapshot.graph.tracks.push_back(consumer);
        }
        snapshot.graph.tracks.push_back(highLatencyTrack);

        TimelineEngine engine;
        juce::String error;
        ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;

        // Assert: normal route PDC remains responsible for the full dependent or automated path.
        const auto* graph = TimelineEngineTestPeer::committedGraph(engine);
        ASSERT_NE(graph, nullptr);
        ASSERT_GE(graph->summary.maximumLatencySamples, 24u);
        const auto monitored =
            std::find_if(graph->tracks.begin(), graph->tracks.end(),
                         [](const auto& track) { return track->id == "track:monitored"; });
        ASSERT_NE(monitored, graph->tracks.end());
        EXPECT_TRUE((*monitored)->runtime->lowLatencyMonitoring());
        EXPECT_FALSE((*monitored)->runtime->lowLatencyPdcBypassActive());
        EXPECT_EQ((*monitored)->runtime->timelinePdcDelaySamples, 0);
    }
}

TEST(MixerRoutingTest, WarmsAndResetsBusReturnAndMasterEffects) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    MixBusSpec group;
    group.id = "bus:group";
    auto returnBus = group;
    returnBus.id = "bus:return";
    returnBus.kind = MixBusKindSpec::returnBus;
    snapshot.graph.mixer.buses = {group, returnBus};
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error)) << error;
    auto* graph = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_NE(graph, nullptr);
    ASSERT_EQ(graph->mixer.buses.size(), 2u);

    std::array<ProcessorTrace, 3> traces;
    for (std::size_t index = 0; index < graph->mixer.buses.size(); ++index)
        ASSERT_TRUE(TimelineEngineTestPeer::addChainDevice(
            *graph->mixer.buses[index]->effects, "fx:bus:" + juce::String(index),
            std::make_unique<TestProcessor>(traces[index]), 48'000.0, 32, error))
            << error;
    ASSERT_TRUE(TimelineEngineTestPeer::addChainDevice(*graph->mixer.masterEffects, "fx:master",
                                                       std::make_unique<TestProcessor>(traces[2]),
                                                       48'000.0, 32, error))
        << error;
    const std::array<int, 3> resetCounts{traces[0].resetCount, traces[1].resetCount,
                                         traces[2].resetCount};

    // Act
    engine.warmUpPluginDevices(32);

    // Assert: every mixer chain received silence without being reset early.
    for (std::size_t index = 0; index < traces.size(); ++index) {
        EXPECT_EQ(traces[index].processBlockCount, 1);
        EXPECT_FLOAT_EQ(traces[index].inputPeak, 0.0f);
        EXPECT_EQ(traces[index].resetCount, resetCounts[index]);
    }

    // Act: the render boundary resets all warmed plugin state.
    engine.resetPluginDevices();

    // Assert
    for (std::size_t index = 0; index < traces.size(); ++index)
        EXPECT_EQ(traces[index].resetCount, resetCounts[index] + 1);
}

TEST(MixerRoutingTest, LowLatencyMidiMonitoringKeepsTimelineEventsAtPdcOffset) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    auto instrumentTrack = makeInstrumentTrack("track:instrument");
    instrumentTrack.armed = true;
    instrumentTrack.midiClips.push_back(
        MidiClipSpec{"midi:timeline", 0, 960, false, false, {{0, 120, 60, 100, 1}}, {}});
    auto latencyTrack = makeAudioTrack("track:latency");
    latencyTrack.effects = {{"fx:latency:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                            {"fx:latency:2", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                            {"fx:latency:3", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    snapshot.graph.tracks = {instrumentTrack, latencyTrack};
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error)) << error;
    InstrumentTrace trace;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
    ASSERT_NE(rack, nullptr) << error;
    ASSERT_TRUE(rack->prepareTimelineMidiCapacity(2, error)) << error;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:instrument",
                                                               "instrument:live", std::move(rack)));
    ASSERT_EQ(TimelineEngineTestPeer::committedGraph(engine)->summary.maximumLatencySamples, 24u);
    ASSERT_EQ(engine.enqueueTargetedMidi("track:instrument", juce::MidiMessage::noteOn(1, 72, 0.8f),
                                         error),
              RealtimeRequest::accepted)
        << error;

    // Act
    ASSERT_TRUE(engine.play());
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

    // Assert: live MIDI is immediate while the clip remains PDC-aligned.
    EXPECT_EQ(trace.midiSamplePositions, (std::vector<int>{0, 24}));
}

TEST(MixerRoutingTest, DisconnectedBusEffectsDoNotChangeMasterLatency) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:source");
    snapshot.graph.tracks.push_back(track);
    MixBusSpec group;
    group.id = "bus:group";
    group.effects = {{"fx:group", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    auto returnBus = group;
    returnBus.id = "bus:return";
    returnBus.kind = MixBusKindSpec::returnBus;
    returnBus.effects.front().id = "fx:return";
    snapshot.graph.mixer.buses = {group, returnBus};
    snapshot.graph.mixer.sends = {
        {"send:source", track.id, returnBus.id, 0.0, SendPositionSpec::postFader, false}};

    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    auto* graph = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_NE(graph, nullptr);
    EXPECT_EQ(graph->summary.maximumLatencySamples, 0u);

    // Act: connect the Group and enable the Return Send.
    snapshot.graph.tracks[0].outputBusId = group.id;
    snapshot.graph.mixer.sends[0].enabled = true;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    graph = TimelineEngineTestPeer::committedGraph(engine);

    // Assert: either connected Bus effect contributes eight samples.
    ASSERT_NE(graph, nullptr);
    EXPECT_EQ(graph->summary.maximumLatencySamples, 8u);

    // Act: Mute and Solo change audibility, not the structural PDC paths.
    snapshot.graph.tracks[0].muted = true;
    snapshot.graph.mixer.buses[0].muted = true;
    snapshot.graph.mixer.buses[1].solo = true;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    graph = TimelineEngineTestPeer::committedGraph(engine);

    // Assert
    ASSERT_NE(graph, nullptr);
    EXPECT_EQ(graph->summary.maximumLatencySamples, 8u);
}

}  // namespace riffra
