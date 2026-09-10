#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, KeepsAudioCaptureOpenForTheWholeAudioCallback) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    constexpr int kBlockSamples = 512;
    ASSERT_TRUE(engine.loadSnapshot(makeAudioTrackSnapshot(10, false, true), formats, 48'000.0,
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
    ASSERT_TRUE(engine.startRecording(0, error));
    ASSERT_TRUE(engine.recordingWindow(kBlockSamples, captureOffset, captureSamples));

    // Act
    engine.mix(inputChannels.data(), 1, outputChannels.data(), 2, kBlockSamples);
    const auto rawSamplesAfterCallback = captureSink.totalRawSamples;
    const auto beginCountAfterCallback = captureSink.beginCount;
    const auto endCountAfterCallback = captureSink.endCount;
    engine.stopRecording();
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
    ASSERT_TRUE(engine.loadSnapshot(makeAudioTrackSnapshot(1, false, true), formats, 48'000.0,
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
    ASSERT_TRUE(engine.startRecording(0, error));
    ASSERT_TRUE(engine.recordingWindow(kRecordingSamples, captureOffset, captureSamples));

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

}  // namespace riffra
