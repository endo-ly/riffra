#include <gtest/gtest.h>

#include <array>
#include <atomic>
#include <chrono>
#include <cstdlib>
#include <thread>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, UsesTempoAndMeterAtTheRecordingPositionForCountIn) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto snapshot = makeAudioTrackSnapshot(1, false, true);
    snapshot.graph.timebase.tempoChanges.push_back({1920, 90.0});
    snapshot.graph.timebase.timeSignatureChanges.push_back({1920, 3, 8});
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
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, 257, captureOffset, captureSamples));
    EXPECT_EQ(captureOffset, 1);
    EXPECT_EQ(captureSamples, 256);
}

TEST(TimelineEngineTest, KeepsAudioCaptureOpenForTheWholeAudioCallback) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(10, false, true), formats, 48'000.0,
                                 kBlockSamples, error));
    CaptureIsolationSink captureSink;
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
    ASSERT_TRUE(engine.stopRecording(error));
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
    auto cancelled = pipeline.takeFinalizedRecording();
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
    auto finalized = pipeline.takeFinalizedRecording();
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
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
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
    ASSERT_TRUE(engine.stopRecording(error));
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
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
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
    ASSERT_TRUE(engine.stopRecording(error));
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
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
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
    }
    const auto beforeStop = callback.recording().status();
    juce::String stopError;
    ASSERT_EQ(callback.recording().stop(stopError), RealtimeRequest::accepted) << stopError;
    auto detached = callback.takeFinalizedRecording();
    const auto processed = detached != nullptr &&
                           engine.processFinalizedRecording(detached.get(), stopError);
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

TEST(TimelineEngineTest, StreamsALongLoopRecordingWithoutPreallocatingRam) {
    // A short loop with many passes used to exceed the pre-allocated capture buffer.
    // It runs as a Slow Test outside the regular pull-request verification.
    if (std::getenv("RIFFRA_RUN_SLOW_TESTS") == nullptr) {
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
    ASSERT_TRUE(
        TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
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
    ASSERT_TRUE(engine.stopRecording(error));
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
