#include <gtest/gtest.h>

#include <tuple>

#include "support/TimelineTestSupport.h"

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

TEST(TimelineEngineTest, CapturesAlignedPostFaderStemsAcrossSplitBlocks) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("source.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    snapshot.graph.timebase.tempoChanges.push_back({21, 100.0});
    auto first = makeAudioTrack("first");
    first.panLaw = "unityCenterStereo";
    first.gainDb = -6.0;
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
    second.id = "second";
    second.gainDb = 0.0;
    second.audioClips[0].id = "clip:second";
    second.effects = {{"fx:one", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}},
                      {"fx:two", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    snapshot.graph.tracks = {first, second};
    snapshot.graph.mixer.masterEffects = {{"fx:master", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}}};
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 64, error)) << error;
    ASSERT_TRUE(engine.prepareOutputCapture(
        {{OfflineCaptureKind::track, "first"}, {OfflineCaptureKind::track, "second"}}, error))
        << error;
    ASSERT_TRUE(engine.play());
    juce::AudioBuffer<float> mix(2, 64);
    std::array<std::vector<float>, 2> stems;
    for (int block = 0; block < 12; ++block) {
        mix.clear();
        engine.mix(mix.getArrayOfWritePointers(), 2, 64);
        for (std::size_t i = 0; i < stems.size(); ++i) {
            const auto& buffer = engine.captureBuffer(i);
            stems[i].insert(stems[i].end(), buffer.getReadPointer(0),
                            buffer.getReadPointer(0) + 64);
        }
    }
    const auto firstSound = [](const std::vector<float>& samples) {
        return std::find_if(samples.begin(), samples.end(),
                            [](float sample) { return std::abs(sample) > 0.001f; }) -
               samples.begin();
    };
    EXPECT_EQ(firstSound(stems[0]), firstSound(stems[1]));
    EXPECT_LT(firstSound(stems[0]), 700);
    EXPECT_GT(std::abs(stems[0][560]), 0.01f);
    // Each test effect has a default gain of one half.
    EXPECT_NEAR(stems[0][560] / stems[1][560], 4.0f * juce::Decibels::decibelsToGain(-6.0f), 0.01f);
}

TEST(TimelineEngineTest, OfflineCaptureWritesTrackBusesAndMasterFromOneGraph) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("capture-source.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 8'192, 8'192));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("capture-track");
    track.panLaw = "unityCenterStereo";
    track.outputBusId = "group";
    track.gainDb = -6.0;
    AudioClipSpec clip;
    clip.id = "clip:capture";
    clip.path = source.getFullPathName();
    clip.sourceSampleRate = 48'000;
    clip.sourceEndFrame = 8'192;
    clip.durationFrames = 8'192;
    clip.durationSampleRate = 48'000;
    track.audioClips.push_back(clip);
    snapshot.graph.tracks = {track};
    snapshot.graph.mixer.buses = {
        {"group", MixBusKindSpec::group, -6.0, 0.0, "unityCenterStereo", false, false, {}},
        {"return", MixBusKindSpec::returnBus, -12.0, 0.0, "unityCenterStereo", false, false, {}}};
    snapshot.graph.mixer.sends = {
        {"send", track.id, "return", 0.0, SendPositionSpec::postFader, true}};
    snapshot.graph.masterGainDb = -6.0;
    OfflineRenderRequestSpec request;
    request.graph = snapshot.graph;
    request.startTick = 0;
    request.endTick = 960;
    request.sampleRate = 48'000;
    request.blockSize = 64;
    for (const auto& [kind, id, name] :
         std::vector<std::tuple<OfflineCaptureKind, juce::String, juce::String>>{
             {OfflineCaptureKind::track, track.id, "track.wav"},
             {OfflineCaptureKind::bus, "group", "group.wav"},
             {OfflineCaptureKind::bus, "return", "return.wav"},
             {OfflineCaptureKind::master, {}, "master.wav"}})
        request.captures.push_back(
            {{kind, id}, directory.get().getChildFile(name).getFullPathName()});
    juce::String error;
    auto renderer = OfflineRenderer::prepare(request, formats, error);
    ASSERT_NE(renderer, nullptr) << error;
    OfflineRenderer::Result result;
    ASSERT_TRUE(renderer->render(formats, result, error)) << error;
    std::vector<float> values;
    for (const auto& capture : request.captures) {
        auto reader = std::unique_ptr<juce::AudioFormatReader>(
            formats.createReaderFor(juce::File(capture.destination)));
        ASSERT_NE(reader, nullptr);
        EXPECT_EQ(reader->lengthInSamples, static_cast<std::int64_t>(result.frames));
        juce::AudioBuffer<float> samples(2, 1);
        ASSERT_TRUE(reader->read(&samples, 0, 1, 256, true, true));
        values.push_back(samples.getSample(0, 0));
    }
    ASSERT_GT(values[0], 0.01f);
    const auto groupGain = juce::Decibels::decibelsToGain(-6.0f);
    const auto returnGain = juce::Decibels::decibelsToGain(-12.0f);
    EXPECT_NEAR(values[1], values[0] * groupGain, 1.0e-5f);
    EXPECT_NEAR(values[2], values[0] * returnGain, 1.0e-5f);
    EXPECT_NEAR(values[3], (values[1] + values[2]) * groupGain, 1.0e-5f);

    auto mixdown = request;
    mixdown.captures.clear();
    mixdown.destination = directory.get().getChildFile("mixdown.wav").getFullPathName();
    auto baseline = OfflineRenderer::prepare(mixdown, formats, error);
    ASSERT_NE(baseline, nullptr) << error;
    ASSERT_TRUE(baseline->render(formats, result, error)) << error;
    auto reader = std::unique_ptr<juce::AudioFormatReader>(
        formats.createReaderFor(juce::File(mixdown.destination)));
    ASSERT_NE(reader, nullptr);
    juce::AudioBuffer<float> sample(2, 1);
    ASSERT_TRUE(reader->read(&sample, 0, 1, 256, true, true));
    EXPECT_FLOAT_EQ(sample.getSample(0, 0), values[3]);

    request.graph.tracks.front().muted = true;
    for (auto& capture : request.captures)
        capture.destination =
            directory.get()
                .getChildFile("muted-" + juce::File(capture.destination).getFileName())
                .getFullPathName();
    auto muted = OfflineRenderer::prepare(request, formats, error);
    ASSERT_NE(muted, nullptr) << error;
    ASSERT_TRUE(muted->render(formats, result, error)) << error;
    for (const auto& capture : request.captures) {
        auto mutedReader = std::unique_ptr<juce::AudioFormatReader>(
            formats.createReaderFor(juce::File(capture.destination)));
        ASSERT_NE(mutedReader, nullptr);
        ASSERT_TRUE(mutedReader->read(&sample, 0, 1, 256, true, true));
        EXPECT_FLOAT_EQ(sample.getSample(0, 0), 0.0f);
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
        // Offline output preserves the attack from the first latency-aligned sample.
        if (block == 0) {
            EXPECT_NEAR(left[20], (0.05f * 2.0f + 0.10f) * 0.5f, 0.002f);
            EXPECT_NEAR(right[20], left[20], 0.002f);
        }
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

TEST(TimelineEngineTest, PreparedAmplitudeAutomationHandlesOverlappingTransitions) {
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, false, false);
    snapshot.graph.automation.push_back(
        {AutomationTargetKind::trackVolume,
         snapshot.graph.tracks.front().id,
         {},
         AutomationInterpolationSpec::linearAmplitude,
         {{0, 0.0}, {960, -6.020599913279624}, {1440, -12.041199826559248}},
         0.5});
    TimelineEngine engine(true);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 100.0, 64, error)) << error;
    const auto* graph = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_NE(graph, nullptr);
    auto cursor = graph->tracks.front()->runtime->volumeAutomation.cursorAt(100);
    EXPECT_NEAR(cursor.valueAt(100, 0.0f), -8.519375f, 0.0001f);
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
                                         .getSiblingFile("sonalloy-instruments")
                                         .getChildFile("duck");
    auto consumer = makeInstrumentTrack("consumer-a");
    consumer.panLaw = "unityCenterStereo";
    consumer.externalAudioSourceTrackIds = {"source"};
    consumer.instrument = InternalInstrumentSpec{
        "instrument:a", instrumentDirectory.getChildFile("definition.json").loadFileAsString(),
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
    auto summedGraph = singleGraph;
    auto extraSource = summedGraph.tracks[2];
    extraSource.id = "source-extra";
    std::get<InternalInstrumentSpec>(*extraSource.instrument).id = "instrument:source-extra";
    for (auto& clip : extraSource.midiClips) clip.id += ":extra";
    // Two half-amplitude sources must produce the same aligned input signal.
    extraSource.gainDb -= 6.020599913279624;
    summedGraph.tracks[2].gainDb -= 6.020599913279624;
    summedGraph.tracks.push_back(extraSource);
    summedGraph.tracks[0].externalAudioSourceTrackIds = {"source", "source-extra"};
    const auto summed = render(summedGraph, "summed-sources");
    for (int channel = 0; channel < 2; ++channel)
        for (int sample = 0; sample < single.getNumSamples(); ++sample)
            EXPECT_NEAR(summed.getSample(channel, sample), single.getSample(channel, sample),
                        1.0e-5f);
    snapshot.graph.tracks[0].externalAudioSourceTrackIds = {"consumer-b"};
    snapshot.graph.tracks[1].externalAudioSourceTrackIds = {"consumer-a"};
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
        {"mono-44100", mono.getFullPathName(), 44'100, 22'050, 44'100, 22'050, 44'100, 0, 0, 0,
         FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false},
        {"stereo-48000", stereo.getFullPathName(), 48'000, 24'000, 48'000, 24'000, 48'000, 0, 0, 0,
         FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false},
    };
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;

    // Act
    ASSERT_TRUE(engine.play());
    std::array<float, 512> left{};
    std::array<float, 512> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    engine.mix(outputs.data(), 2, 512);

    // Assert: both sources contribute from the first block after preparation.
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.1f);
    EXPECT_GT(*std::max_element(right.begin(), right.end()), 0.1f);
}

TEST(TimelineEngineTest, PreparesManyShortClips) {
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("short.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 128, 2'000));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:short");
    for (int index = 0; index < 20; ++index)
        track.audioClips.push_back({"clip:" + juce::String(index), source.getFullPathName(), 48'000,
                                    0, 128, 48'000, 48'000, 0, 0, 0, FadeShapeSpec::linear, 0.0,
                                    0.0, TakeVariantSpec::raw, true, false});
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000, 128, error)) << error;
    ASSERT_TRUE(engine.play());
    std::array<float, 128> left{}, right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    engine.mix(outputs.data(), 2, 128);
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.1f);
}

TEST(TimelineEngineTest, ReadsTheRequestedPositionAfterASeekOutsideReadAhead) {
    test::TemporaryDirectory directory;
    const auto source = directory.get().getChildFile("seek.wav");
    ASSERT_TRUE(writePcmWave(source, 48'000, 1, 144'000, 4'000));
    {
        std::fstream stream(source.getFullPathName().toStdString(),
                            std::ios::in | std::ios::out | std::ios::binary);
        ASSERT_TRUE(stream.is_open());
        stream.seekp(44 + 48'000 * sizeof(std::int16_t));
        const std::int16_t sample = -8'000;
        for (int index = 0; index < 96'000; ++index)
            stream.write(reinterpret_cast<const char*>(&sample), sizeof(sample));
        ASSERT_TRUE(stream.good());
    }
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:seek");
    track.audioClips.push_back({"clip:seek", source.getFullPathName(), 48'000, 0, 144'000, 288'000,
                                48'000, 0, 0, 0, FadeShapeSpec::linear, 0.0, 0.0,
                                TakeVariantSpec::raw, true, false});
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000, 128, error)) << error;
    ASSERT_TRUE(engine.play());
    std::array<float, 128> left{}, right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    engine.mix(outputs.data(), 2, 128);
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.01f);
    ASSERT_TRUE(engine.seekToTick(3840));
    bool reached = false;
    for (int attempt = 0; attempt < 200 && !reached; ++attempt) {
        left.fill(0.0f);
        right.fill(0.0f);
        engine.mix(outputs.data(), 2, 128);
        reached = *std::min_element(left.begin(), left.end()) < -0.1f;
        if (!reached) juce::Thread::sleep(1);
    }
    EXPECT_TRUE(reached);
    ASSERT_TRUE(engine.seekToTick(6720));
    reached = false;
    for (int attempt = 0; attempt < 200 && !reached; ++attempt) {
        left.fill(0.0f);
        right.fill(0.0f);
        engine.mix(outputs.data(), 2, 128);
        reached = *std::max_element(left.begin(), left.end()) > 0.05f;
        if (!reached) juce::Thread::sleep(1);
    }
    EXPECT_TRUE(reached);
}

}  // namespace riffra
