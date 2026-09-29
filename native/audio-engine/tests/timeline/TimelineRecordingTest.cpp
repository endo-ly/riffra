#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <thread>

#include "TimelineTestSupport.h"

namespace riffra {

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

}  // namespace riffra
