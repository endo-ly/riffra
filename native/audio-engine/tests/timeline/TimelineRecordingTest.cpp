#include <gtest/gtest.h>

#include <array>
#include <atomic>
#include <chrono>
#include <cstdlib>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, UsesTempoAndMeterAtTheRecordingPositionForCountIn) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, false, true);
    snapshot.graph.timebase.tempoChanges.push_back({1920, 90.0});
    snapshot.graph.timebase.timeSignatureChanges.push_back({1920, 3, 8});
    snapshot.graph.metronomeEnabled = true;
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000, 256, error));
    ASSERT_TRUE(engine.seekToTick(1920));
    ASSERT_EQ(engine.startRecording(1, error), RealtimeRequest::accepted);

    // Act / Assert: one eighth-note at 90 BPM is 16,000 samples.
    int captureOffset = 0;
    int captureSamples = 0;
    EXPECT_FALSE(
        TimelineEngineTestPeer::recordingWindow(engine, 15'999, captureOffset, captureSamples));
    EXPECT_EQ(captureSamples, 0);
    std::vector<float> countInClick(15'999);
    const std::array<float*, 1> clickOutput{countInClick.data()};
    engine.mixMetronome(clickOutput.data(), 1, static_cast<int>(countInClick.size()));
    EXPECT_GT(*std::max_element(countInClick.begin(), countInClick.end()), 0.1f);
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, 257, captureOffset, captureSamples));
    EXPECT_EQ(captureOffset, 1);
    EXPECT_EQ(captureSamples, 256);
}

TEST(TimelineEngineTest, PunchCaptureMatchesItsTimelineRange) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, false, true);
    snapshot.graph.punchRange = TickRangeSpec{480, 960};
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    ASSERT_TRUE(engine.seekToTick(0));
    ASSERT_EQ(engine.startRecording(0, error), RealtimeRequest::accepted);

    // Act: request a block extending beyond the punch end.
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, 24'512, captureOffset, captureSamples));

    // Assert: ticks 480..960 map to the exact 12,000-sample capture range.
    EXPECT_EQ(captureOffset, 12'000);
    EXPECT_EQ(captureSamples, 12'000);
}

TEST(TimelineEngineTest, KeepsAudioCaptureOpenForTheWholeAudioCallback) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    constexpr int kBlockSamples = 512;
    const auto sourceFile = directory.get().getChildFile("timeline-playback.wav");
    ASSERT_TRUE(writePcmWave(sourceFile, 48'000, 1, kBlockSamples, 16'384));
    auto snapshot = makeAudioTrackSnapshot(10, false, true);
    const auto playbackClip = makeRawAndProcessedClipSnapshot(sourceFile, sourceFile, kBlockSamples)
                                  .graph.tracks.front()
                                  .audioClips.front();
    snapshot.graph.tracks.front().audioClips.push_back(playbackClip);
    snapshot.graph.tracks.front().gainDb = 12.0;
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, kBlockSamples, error));
    CaptureIsolationSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputChannels{input.data()};
    const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE((engine.startRecording(0, error) == RealtimeRequest::accepted));
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kBlockSamples, captureOffset,
                                                        captureSamples));

    // Act
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
    const auto rawSamplesAfterCallback = captureSink.totalRawSamples;
    const auto beginCountAfterCallback = captureSink.beginCount;
    const auto endCountAfterCallback = captureSink.endCount;
    const auto finalized = finalizeCapturedRecording(engine, error);
    engine.clearRecordingSink();

    // Assert
    EXPECT_TRUE(finalized);
    EXPECT_EQ(captureOffset, 0);
    EXPECT_EQ(captureSamples, kBlockSamples);
    EXPECT_EQ(rawSamplesAfterCallback, kBlockSamples);
    EXPECT_EQ(beginCountAfterCallback, 1);
    EXPECT_EQ(endCountAfterCallback, 0);
    EXPECT_EQ(captureSink.totalRawSamples, kBlockSamples);
    EXPECT_EQ(captureSink.beginCount, 1);
    EXPECT_EQ(captureSink.endCount, 1);
    EXPECT_GT(outputLeft[400], 0.1f);

    const auto rawFile = directory.get().getChildFile("capture-isolation-raw.wav");
    auto rawReader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(rawFile));
    ASSERT_NE(rawReader, nullptr);
    juce::AudioBuffer<float> capturedInput(1, kBlockSamples);
    ASSERT_TRUE(rawReader->read(&capturedInput, 0, kBlockSamples, 0, true, false));
    EXPECT_NEAR(capturedInput.getSample(0, 400), 0.05f, 0.001f);
}

TEST(TimelineEngineTest, StreamsOfflineProcessingForASingleLongRecordingSegment) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    constexpr int kBlockSamples = 512;
    constexpr int kRecordingSamples = 100'000;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, false, true), formats, 48'000.0,
                                 kBlockSamples, error));
    test::TemporaryDirectory directory;
    CaptureIsolationSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputChannels{input.data()};
    const std::array<float*, 2> outputChannels{outputLeft.data(), outputRight.data()};
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE((engine.startRecording(0, error) == RealtimeRequest::accepted));
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kRecordingSamples, captureOffset,
                                                        captureSamples));

    // Act
    auto remaining = kRecordingSamples;
    while (remaining > 0) {
        const auto block = std::min(kBlockSamples, remaining);
        engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, block);
        remaining -= block;
    }
    const auto finalized = finalizeCapturedRecording(engine, error);
    engine.clearRecordingSink();

    // Assert
    EXPECT_TRUE(finalized);
    EXPECT_EQ(captureOffset, 0);
    EXPECT_EQ(captureSamples, kRecordingSamples);
    EXPECT_EQ(captureSink.beginCount, 1);
    EXPECT_EQ(captureSink.endCount, 1);
    EXPECT_EQ(captureSink.totalRawSamples, kRecordingSamples);
    EXPECT_EQ(captureSink.totalProcessedSamples, kRecordingSamples);
    EXPECT_GT(captureSink.offlineProcessedWriteCalls, 1);
    EXPECT_LE(captureSink.maxOfflineProcessedWriteSize, kBlockSamples);
}

TEST(TimelineRecordingTest, RecordingStartQueueFullLeavesNoSessionOrSinkAndCanRetry) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    AudioRenderPipeline pipeline(engine);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, false, true), formats, 48'000.0,
                                 512, error))
        << error;
    engine.setRealtimeOwner(RealtimeOwner::audio);
    for (std::size_t command = 0; command < TimelineEngineTestPeer::realtimeCommandCapacity();
         ++command)
        ASSERT_TRUE(engine.play());
    test::TemporaryDirectory directory;

    // Act
    const auto failedStart = pipeline.recording().start(directory.get(), 0, error);

    // Assert
    EXPECT_EQ(failedStart, RealtimeRequest::queueFull);
    EXPECT_FALSE(pipeline.recordingStatus().active);
    EXPECT_FALSE(TimelineEngineTestPeer::hasRecordingSink(engine));
    EXPECT_FALSE(directory.get().exists());

    // Retry the same request after the queue is available.
    engine.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_EQ(pipeline.recording().start(directory.get(), 0, error), RealtimeRequest::accepted);
    EXPECT_TRUE(pipeline.recordingStatus().active);
    EXPECT_TRUE(TimelineEngineTestPeer::hasRecordingSink(engine));
    ASSERT_EQ(pipeline.recording().stop(error), RealtimeRequest::accepted);
    auto cancelled = pipeline.recording().takePendingFinalization();
    ASSERT_NE(cancelled, nullptr);
    EXPECT_TRUE(cancelled->cancel(error));
}

TEST(TimelineRecordingTest, RecordingStopQueueFullLeavesCaptureActiveAndCanRetry) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    AudioRenderPipeline pipeline(engine);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, false, true), formats, 48'000.0,
                                 512, error))
        << error;
    test::TemporaryDirectory directory;
    ASSERT_EQ(pipeline.recording().start(directory.get(), 0, error), RealtimeRequest::accepted);
    engine.setRealtimeOwner(RealtimeOwner::audio);
    for (std::size_t command = 0; command < TimelineEngineTestPeer::realtimeCommandCapacity();
         ++command)
        ASSERT_TRUE(engine.play());

    // Act
    const auto failedStop = pipeline.recording().stop(error);

    // Assert
    EXPECT_EQ(failedStop, RealtimeRequest::queueFull);
    EXPECT_TRUE(pipeline.recordingStatus().active);
    EXPECT_TRUE(TimelineEngineTestPeer::hasRecordingSink(engine));
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::recording);

    engine.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_EQ(pipeline.recording().stop(error), RealtimeRequest::accepted);
    EXPECT_FALSE(pipeline.recordingStatus().active);
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::idle);
    EXPECT_EQ(engine.status().frame.transportState, TransportState::stopped);
    EXPECT_FALSE(TimelineEngineTestPeer::hasRecordingSink(engine));
    auto finalized = pipeline.recording().takePendingFinalization();
    ASSERT_NE(finalized, nullptr);
    EXPECT_TRUE(finalized->cancel(error));
}

TEST(TimelineRecordingTest, CountInStopUsesTheLastQueueSlotAsOneOperation) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    AudioRenderPipeline pipeline(engine);
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, false, true), formats, 48'000.0,
                                 512, error))
        << error;
    test::TemporaryDirectory directory;
    ASSERT_EQ(pipeline.recording().start(directory.get(), 1, error), RealtimeRequest::accepted);
    ASSERT_EQ(engine.status().frame.recordingPhase, RecordingPhase::countingIn);
    engine.setRealtimeOwner(RealtimeOwner::audio);
    for (std::size_t command = 0; command + 1 < TimelineEngineTestPeer::realtimeCommandCapacity();
         ++command)
        ASSERT_TRUE(engine.play());

    const auto expectedNextSequence = TimelineEngineTestPeer::nextCommandSequence(engine) + 1;
    std::atomic<bool> stopFinished{false};
    std::atomic<RealtimeRequest> stopResult{RealtimeRequest::rejected};
    juce::String stopError;
    std::thread stopper([&] {
        stopResult.store(pipeline.recording().stop(stopError), std::memory_order_release);
        stopFinished.store(true, std::memory_order_release);
    });

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    auto submitted = false;
    while (!stopFinished.load(std::memory_order_acquire) &&
           std::chrono::steady_clock::now() < deadline) {
        if (TimelineEngineTestPeer::nextCommandSequence(engine) >= expectedNextSequence) {
            submitted = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }

    // Act
    engine.setRealtimeOwner(RealtimeOwner::control);
    stopper.join();

    // Assert
    EXPECT_TRUE(submitted);
    EXPECT_EQ(stopResult.load(std::memory_order_acquire), RealtimeRequest::accepted);
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::idle);
    EXPECT_EQ(engine.status().frame.transportState, TransportState::stopped);
    EXPECT_TRUE(pipeline.recordingStatus().cancelled);
    EXPECT_FALSE(pipeline.recordingStatus().active);
    EXPECT_FALSE(TimelineEngineTestPeer::hasRecordingSink(engine));
}

TEST(TimelineEngineTest, ClosesNonOverlappingSegmentsForALoopCapture) {
    // Arrange: an armed Track with a one-bar loop.
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.graph.loopRange = {true, 0, 960};
    snapshot.graph.metronomeEnabled = true;
    auto track = makeAudioTrack("track:loop");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    test::TemporaryDirectory directory;
    CaptureIsolationSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);

    constexpr int kPassSamples = 24'000;
    constexpr int kBlockSamples = 512;
    constexpr int kTotalSamples = kPassSamples * 3;
    ASSERT_TRUE(engine.seekToTick(0));
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(engine.startRecording(0, error) == RealtimeRequest::accepted);
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kBlockSamples> input{};
    input.fill(0.05f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    auto remaining = kTotalSamples;
    while (remaining > kBlockSamples) {
        engine.mix(inputs.data(), 1, outputs.data(), 2, kBlockSamples);
        remaining -= kBlockSamples;
    }
    engine.mix(inputs.data(), 1, outputs.data(), 2, remaining);
    ASSERT_TRUE(finalizeCapturedRecording(engine, error));
    engine.clearRecordingSink();

    // Assert: the capture closes once per pass with no overlap between segments.
    ASSERT_EQ(captureSink.beginCount, 3);
    ASSERT_EQ(captureSink.endCount, 3);
    EXPECT_EQ(captureSink.loopBoundaryCount, 3);
    EXPECT_EQ(captureSink.totalRawSamples, kTotalSamples);
    EXPECT_EQ(captureSink.totalProcessedSamples, kTotalSamples);
    EXPECT_GT(captureSink.offlineProcessedWriteCalls, 1);
    EXPECT_LE(captureSink.maxOfflineProcessedWriteSize, kBlockSamples);
    for (int index = 0; index < captureSink.endCount; ++index) {
        const auto segment = static_cast<std::size_t>(index);
        EXPECT_GT(captureSink.segmentRawSamples[segment], 0);
        EXPECT_GT(captureSink.endAudioSamples[segment], captureSink.beginAudioSamples[segment]);
        if (index > 0)
            EXPECT_GE(captureSink.beginAudioSamples[segment],
                      captureSink.endAudioSamples[segment - 1]);
        EXPECT_EQ(captureSink.beginTimelineSamples[segment], 0u);
        EXPECT_EQ(captureSink.endTimelineSamples[segment],
                  static_cast<std::uint64_t>(kPassSamples));
    }
}

TEST(TimelineEngineTest, KeepsRawAndProcessedAlignedAcrossLoopPasses) {
    // Arrange: an armed Track with a one-bar loop and one impulse per pass.
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.graph.loopRange = {true, 0, 960};
    auto track = makeAudioTrack("track:loop");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    test::TemporaryDirectory directory;
    LoopDataCaptureSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);

    constexpr int kPassSamples = 24'000;
    constexpr int kPasses = 3;
    constexpr int kTotalSamples = kPassSamples * kPasses;
    constexpr int kBlockSamples = 512;
    constexpr float kImpulseAmplitude = 0.9f;
    constexpr std::array<int, 3> kImpulsePositions{256, 1'256, 2'256};
    ASSERT_TRUE(engine.seekToTick(0));
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(engine.startRecording(0, error) == RealtimeRequest::accepted);
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kBlockSamples> input{};
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    int mixed = 0;
    while (mixed < kTotalSamples) {
        const auto block = std::min(kBlockSamples, kTotalSamples - mixed);
        const auto passIndex = mixed / kPassSamples;
        const auto positionInPass = mixed % kPassSamples;
        const auto impulse =
            static_cast<std::size_t>(kImpulsePositions[static_cast<std::size_t>(passIndex)]);
        input.fill(0.0f);
        if (passIndex < kPasses && positionInPass <= static_cast<int>(impulse) &&
            static_cast<int>(impulse) < positionInPass + block)
            input[impulse - static_cast<std::size_t>(positionInPass)] = kImpulseAmplitude;
        engine.mix(inputs.data(), 1, outputs.data(), 2, block);
        mixed += block;
    }
    ASSERT_TRUE(finalizeCapturedRecording(engine, error));
    engine.clearRecordingSink();

    // Assert: each pass keeps its own impulse and no pass leaks into another.
    ASSERT_EQ(captureSink.segmentCount, kPasses);
    EXPECT_EQ(captureSink.boundaryCount, kPasses);
    ASSERT_EQ(captureSink.totalRaw, kTotalSamples);
    ASSERT_EQ(captureSink.totalProcessed, kTotalSamples);
    ASSERT_EQ(captureSink.rawBuffer.size(), static_cast<std::size_t>(kTotalSamples));
    ASSERT_EQ(captureSink.processedLeft.size(), static_cast<std::size_t>(kTotalSamples));
    for (int pass = 0; pass < kPasses; ++pass) {
        const auto base = static_cast<std::size_t>(pass * kPassSamples);
        const auto impulse =
            static_cast<std::size_t>(kImpulsePositions[static_cast<std::size_t>(pass)]);
        ASSERT_LT(base + impulse, captureSink.rawBuffer.size());
        EXPECT_NEAR(captureSink.rawBuffer[base + impulse], kImpulseAmplitude, 0.001f);
        // The offline chain is empty for this capture, so the processed variant
        // keeps the raw impulse position.
        EXPECT_NEAR(captureSink.processedLeft[base + impulse], kImpulseAmplitude, 0.001f);
        for (int sample = 0; sample < kPassSamples; ++sample) {
            if (static_cast<std::size_t>(sample) == impulse) continue;
            const auto index = base + static_cast<std::size_t>(sample);
            if (index < captureSink.processedLeft.size())
                EXPECT_LT(std::abs(captureSink.processedLeft[index]), 0.001f);
        }
    }
    EXPECT_GT(captureSink.offlineProcessedWriteCalls, 1);
    EXPECT_LE(captureSink.maxOfflineProcessedWriteSize, kBlockSamples);
}

TEST(TimelineEngineTest, CompletesAProductionThreadedWriterRecording) {
    // Arrange: four bars of loop, recorded through the production writer.
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.revision = 10;
    snapshot.graph.loopRange = {true, 0, 15'360};
    auto track = makeAudioTrack("track:prod");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    test::TemporaryDirectory directory;
    const auto destination = directory.get().getChildFile("prod-writer");

    constexpr int kLoopSamples = 384'000;
    constexpr int kPasses = 3;
    constexpr int kTotalSamples = kLoopSamples * kPasses;
    constexpr int kBlockSamples = 512;

    // Act: AudioRenderPipeline owns the transport stop and the capture detachment,
    // and the lifecycle worker owns the offline processing and the session finish.
    ASSERT_TRUE(engine.seekToTick(0));
    AudioRenderPipeline callback(engine);
    juce::String sessionError;
    ASSERT_EQ(callback.recording().start(destination, 0, sessionError), RealtimeRequest::accepted);
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kBlockSamples> input{};
    input.fill(0.06f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    int mixed = 0;
    while (mixed < kTotalSamples) {
        const auto block = std::min(kBlockSamples, kTotalSamples - mixed);
        engine.mix(inputs.data(), 1, outputs.data(), 2, block);
        mixed += block;
        // The production writer is asynchronous and consumes at the device's
        // realtime rate; pace this simulated callback so its bounded queue can drain.
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    const auto beforeStop = callback.recording().status();
    juce::String stopError;
    ASSERT_EQ(callback.recording().stop(stopError), RealtimeRequest::accepted) << stopError;
    auto detached = callback.recording().takePendingFinalization();
    const auto processed =
        detached != nullptr && engine.processFinalizedRecording(detached.get(), stopError);
    juce::String finishError;
    const auto finished = detached != nullptr && detached->finish(processed, finishError);

    // Assert
    EXPECT_TRUE(processed) << stopError;
    EXPECT_TRUE(finished) << finishError;
    const auto rawFile = destination.getChildFile("tracks/0000/raw.wav");
    const auto processedFile = destination.getChildFile("tracks/0000/processed.wav");
    EXPECT_TRUE(rawFile.existsAsFile());
    EXPECT_TRUE(processedFile.existsAsFile());
    auto rawReader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(rawFile));
    auto processedReader =
        std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(processedFile));
    ASSERT_NE(rawReader, nullptr);
    ASSERT_NE(processedReader, nullptr);
    EXPECT_EQ(rawReader->lengthInSamples, kTotalSamples);
    EXPECT_EQ(processedReader->lengthInSamples, kTotalSamples);
    const auto manifest =
        juce::JSON::parse(destination.getChildFile("manifest.json").loadFileAsString());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
    EXPECT_EQ(beforeStop.processedMissingSamples, 0u);
}

TEST(TimelineEngineTest, ReplaysCapturedSidechainForProcessedTakesAndKeepsRawWhenUnavailable) {
    for (const auto replayable : {true, false}) {
        // Arrange: the auxiliary source is a live input, with a different signal on each pass.
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine(true);
        juce::String error;
        auto snapshot = makeTestSnapshot();
        snapshot.graph.loopRange = {true, 0, 48};
        auto source = makeAudioTrack("source");
        source.monitorInput = true;
        source.audioInput = AudioInputSpec{1};
        source.panLaw = "unityCenterStereo";
        auto target = makeAudioTrack("target");
        target.armed = true;
        target.monitorInput = true;
        target.audioInput = AudioInputSpec{0};
        target.panLaw = "unityCenterStereo";
        target.effects.push_back({"fx:target", RIFFRA_TEST_SIDECHAIN_BUNDLE_PATH, {}});
        snapshot.graph.tracks = {source, target};
        snapshot.graph.mixer.sidechainSends = {
            {"send:aux", "source", "fx:target", 0.0, SendPositionSpec::postFader, true}};
        ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000, 64, error)) << error;
        test::TemporaryDirectory directory;
        const auto destination = directory.get().getChildFile("capture");
        AudioRenderPipeline pipeline(engine);
        ASSERT_EQ(pipeline.recording().start(destination, 0, error), RealtimeRequest::accepted);
        int offset = 0, count = 0;
        ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, 2537, offset, count));
        std::array<float, 64> raw{}, auxiliary{}, left{}, right{};
        raw.fill(0.05f);
        const std::array<const float*, 2> inputs{raw.data(), auxiliary.data()};
        const std::array<float*, 2> outputs{left.data(), right.data()};

        // Act
        for (int position = 0; position < 2537; position += 64) {
            auxiliary.fill(position < 1200 ? 0.2f : 0.4f);
            engine.mix(inputs.data(), 2, outputs.data(), 2, std::min(64, 2537 - position));
        }
        ASSERT_EQ(pipeline.recording().stop(error), RealtimeRequest::accepted) << error;
        auto capture = pipeline.recording().takePendingFinalization();
        ASSERT_NE(capture, nullptr);
        if (!replayable) {
            const auto auxiliaryFile = capture->prepareSidechainForReading("target", "fx:target");
            ASSERT_TRUE(auxiliaryFile.deleteFile());
        }
        ASSERT_TRUE(engine.processFinalizedRecording(capture.get(), error)) << error;
        ASSERT_TRUE(capture->finish(true, error)) << error;

        // Assert: raw capture is complete in both cases; missing auxiliary never yields a fake
        // variant.
        const auto folder = destination.getChildFile("tracks/0000");
        auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
            formats.createReaderFor(folder.getChildFile("raw.wav")));
        ASSERT_NE(rawReader, nullptr);
        EXPECT_EQ(rawReader->lengthInSamples, 2537);
        const auto manifest =
            juce::JSON::parse(destination.getChildFile("manifest.json").loadFileAsString());
        EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
        const auto trackManifest = manifest.getProperty("tracks", {})[0];
        if (replayable) {
            auto reader = std::unique_ptr<juce::AudioFormatReader>(
                formats.createReaderFor(folder.getChildFile("processed.wav")));
            ASSERT_NE(reader, nullptr);
            ASSERT_EQ(reader->lengthInSamples, 2537);
            juce::AudioBuffer<float> processed(2, 2537);
            ASSERT_TRUE(reader->read(&processed, 0, 2537, 0, true, true));
            EXPECT_NEAR(processed.getSample(0, 600), 0.25f, 0.001f);
            EXPECT_NEAR(processed.getSample(1, 1800), 0.45f, 0.001f);
        } else {
            EXPECT_FALSE(trackManifest.hasProperty("processedFile"));
            EXPECT_TRUE(trackManifest.getProperty("processedUnavailableReason", {})
                            .toString()
                            .contains("fx:target"));
            EXPECT_FALSE(folder.getChildFile("processed.wav").existsAsFile());
        }
    }
}

TEST(TimelineEngineTest, ProductionWriterKeepsPartialLoopSegmentsAlignedInManifest) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeTestSnapshot();
    snapshot.graph.loopRange = {true, 0, 960};
    auto track = makeAudioTrack("track:partial");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));

    TimelineEngine engine;
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, kBlockSamples, error))
        << error;
    ASSERT_TRUE(engine.seekToTick(480));
    test::TemporaryDirectory directory;
    const auto destination = directory.get().getChildFile("partial-loop");
    AudioRenderPipeline callback(engine);
    ASSERT_EQ(callback.recording().start(destination, 0, error), RealtimeRequest::accepted)
        << error;

    constexpr int kTotalSamples = 60'000;
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kBlockSamples> input{};
    input.fill(0.06f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    for (int mixed = 0; mixed < kTotalSamples;) {
        const auto block = std::min(kBlockSamples, kTotalSamples - mixed);
        outputLeft.fill(0.0f);
        outputRight.fill(0.0f);
        engine.mix(inputs.data(), 1, outputs.data(), 2, block);
        mixed += block;
    }
    ASSERT_EQ(callback.recording().stop(error), RealtimeRequest::accepted) << error;
    auto detached = callback.recording().takePendingFinalization();
    ASSERT_NE(detached, nullptr);
    const auto processed = engine.processFinalizedRecording(detached.get(), error);
    juce::String finishError;
    const auto finished = detached->finish(processed, finishError);

    // Assert
    EXPECT_TRUE(processed) << error;
    EXPECT_TRUE(finished) << finishError;
    EXPECT_EQ(captureOffset, 0);
    EXPECT_EQ(captureSamples, kTotalSamples);
    const auto rawFile = destination.getChildFile("tracks/0000/raw.wav");
    const auto processedFile = destination.getChildFile("tracks/0000/processed.wav");
    auto rawReader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(rawFile));
    auto processedReader =
        std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(processedFile));
    ASSERT_NE(rawReader, nullptr);
    ASSERT_NE(processedReader, nullptr);
    EXPECT_EQ(rawReader->lengthInSamples, kTotalSamples);
    EXPECT_EQ(processedReader->lengthInSamples, kTotalSamples);
    juce::AudioBuffer<float> rawAudio(static_cast<int>(rawReader->numChannels), kTotalSamples);
    juce::AudioBuffer<float> processedAudio(static_cast<int>(processedReader->numChannels),
                                            kTotalSamples);
    ASSERT_TRUE(rawReader->read(&rawAudio, 0, kTotalSamples, 0, true, true));
    ASSERT_TRUE(processedReader->read(&processedAudio, 0, kTotalSamples, 0, true, true));
    for (const auto sample : {0, 11'999, 12'000, 35'999, 36'000, 59'999}) {
        EXPECT_NEAR(rawAudio.getSample(0, sample), 0.06f, 0.002f);
        EXPECT_NEAR(processedAudio.getSample(0, sample), 0.06f, 0.002f);
    }

    const auto manifest =
        juce::JSON::parse(destination.getChildFile("manifest.json").loadFileAsString());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
    const auto tracks = manifest.getProperty("tracks", {});
    ASSERT_TRUE(tracks.isArray());
    ASSERT_EQ(tracks.size(), 1);
    const auto segments = tracks[0].getProperty("captureSegments", {});
    ASSERT_TRUE(segments.isArray());
    ASSERT_EQ(segments.size(), 3);
    constexpr std::array<std::pair<juce::int64, juce::int64>, 3> expectedRanges{
        std::pair<juce::int64, juce::int64>{0, 12'000}, {12'000, 36'000}, {36'000, 60'000}};
    for (int index = 0; index < static_cast<int>(expectedRanges.size()); ++index) {
        const auto segment = segments[index];
        const auto [start, end] = expectedRanges[static_cast<std::size_t>(index)];
        EXPECT_EQ(static_cast<juce::int64>(segment.getProperty("rawFileStartSample", -1)), start);
        EXPECT_EQ(static_cast<juce::int64>(segment.getProperty("rawFileEndSample", -1)), end);
        EXPECT_EQ(static_cast<juce::int64>(segment.getProperty("processedFileStartSample", -1)),
                  start);
        EXPECT_EQ(static_cast<juce::int64>(segment.getProperty("processedFileEndSample", -1)), end);
    }
}

TEST(TimelineEngineTest, StreamsALongLoopRecordingWithoutPreallocatingRam) {
    // A short loop with many passes used to exceed the pre-allocated capture buffer.
    // It runs as a Slow Test outside the regular pull-request verification.
    const auto* runSlowTests = std::getenv("RIFFRA_RUN_SLOW_TESTS");
    if (runSlowTests == nullptr || std::string_view(runSlowTests) != "1") {
        GTEST_SKIP() << "set RIFFRA_RUN_SLOW_TESTS=1 to stream a 130-pass loop recording";
    }

    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.graph.loopRange = {true, 0, 192};
    auto track = makeAudioTrack("track:long");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    test::TemporaryDirectory directory;
    LoopDataCaptureSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);

    constexpr int kPassSamples = 4'800;
    constexpr int kPasses = 130;
    constexpr int kTotalSamples = kPassSamples * kPasses;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(engine.seekToTick(0));
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(engine.startRecording(0, error) == RealtimeRequest::accepted);
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kBlockSamples> input{};
    input.fill(0.02f);
    std::array<float, kBlockSamples> outputLeft{};
    std::array<float, kBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    auto remaining = kTotalSamples;
    while (remaining > 0) {
        const auto block = std::min(kBlockSamples, remaining);
        engine.mix(inputs.data(), 1, outputs.data(), 2, block);
        remaining -= block;
    }
    ASSERT_TRUE(finalizeCapturedRecording(engine, error));
    engine.clearRecordingSink();

    // Assert: every pass is recorded, and the processed variant matches the raw total.
    EXPECT_EQ(captureSink.segmentCount, kPasses);
    EXPECT_EQ(captureSink.totalRaw, kTotalSamples);
    EXPECT_EQ(captureSink.totalProcessed, kTotalSamples);
    EXPECT_EQ(captureSink.processedLeft.size(), static_cast<std::size_t>(kTotalSamples));
    EXPECT_GT(captureSink.offlineProcessedWriteCalls, 1);
    EXPECT_LE(captureSink.maxOfflineProcessedWriteSize, kBlockSamples);
}

}  // namespace riffra
