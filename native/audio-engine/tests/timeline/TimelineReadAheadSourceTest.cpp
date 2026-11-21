#include <gtest/gtest.h>

#include "support/TimelineTestSupport.h"

namespace riffra {

TEST(TimelineReadAheadSourceTest, ReportsReadFailures) {
    class FailingReader final : public juce::AudioFormatReader {
    public:
        FailingReader() : AudioFormatReader(nullptr, "failing") {
            lengthInSamples = 1024;
            sampleRate = 48'000;
            numChannels = 1;
            bitsPerSample = 32;
            usesFloatingPointData = true;
        }
        bool readSamples(int* const*, int, int, juce::int64, int) override { return false; }
    };

    juce::TimeSliceThread worker("test read ahead");
    ASSERT_TRUE(worker.startThread());
    TimelineReadAheadSource source(std::make_unique<FailingReader>(), worker, 4096);
    source.prepareToPlay(128, 48'000);
    for (int attempt = 0; attempt < 1000 && !source.failed(); ++attempt) juce::Thread::sleep(1);
    EXPECT_TRUE(source.failed());
    EXPECT_FALSE(source.ready(128));
}

TEST(TimelineReadAheadSourceTest, StreamsHighRateAudioWithoutDroppingBlocks) {
    test::TemporaryDirectory directory;
    const auto path = directory.get().getChildFile("high-rate.wav");
    ASSERT_TRUE(writePcmWave(path, 384'000, 1, 384'000, 8'000));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(path));
    ASSERT_NE(reader, nullptr);
    juce::TimeSliceThread worker("test read ahead");
    ASSERT_TRUE(worker.startThread());
    TimelineReadAheadSource source(std::move(reader), worker, 32'768);
    source.prepareToPlay(384, 384'000);
    for (int attempt = 0; attempt < 1000 && !source.ready(32'000); ++attempt)
        juce::Thread::sleep(1);
    ASSERT_TRUE(source.ready(32'000));
    juce::AudioBuffer<float> block(2, 384);
    for (int index = 0; index < 200; ++index) {
        source.getNextAudioBlock({&block, 0, 384});
        EXPECT_GT(block.findMinMax(0, 0, 384).getStart(), 0.2f) << index;
        juce::Thread::sleep(1);
    }
}

TEST(TimelineReadAheadSourceTest, PreservesTheAvailablePartOfARead) {
    test::TemporaryDirectory directory;
    const auto path = directory.get().getChildFile("partial.wav");
    ASSERT_TRUE(writePcmWave(path, 48'000, 1, 8192, 8'000));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(path));
    ASSERT_NE(reader, nullptr);
    juce::TimeSliceThread worker("test read ahead");
    ASSERT_TRUE(worker.startThread());
    TimelineReadAheadSource source(std::move(reader), worker, 4096);
    source.prepareToPlay(128, 48'000);
    for (int attempt = 0; attempt < 1000 && !source.ready(4096); ++attempt) {
        juce::Thread::sleep(1);
    }
    ASSERT_TRUE(source.ready(4096));
    ASSERT_TRUE(worker.stopThread(1000));
    source.setNextReadPosition(4090);
    juce::AudioBuffer<float> block(2, 16);
    source.getNextAudioBlock({&block, 0, 16});
    EXPECT_GT(block.findMinMax(0, 0, 6).getStart(), 0.2f);
    EXPECT_FLOAT_EQ(block.getMagnitude(0, 6, 10), 0.0f);
}

TEST(TimelineReadAheadSourceTest, TreatsTheEndOfAShortFileAsSilence) {
    test::TemporaryDirectory directory;
    const auto path = directory.get().getChildFile("short-tail.wav");
    ASSERT_TRUE(writePcmWave(path, 48'000, 1, 128, 8'000));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(path));
    ASSERT_NE(reader, nullptr);
    juce::TimeSliceThread worker("test read ahead");
    ASSERT_TRUE(worker.startThread());
    TimelineReadAheadSource source(std::move(reader), worker, 32'768);
    source.prepareToPlay(512, 48'000);
    for (int attempt = 0; attempt < 1000 && !source.ready(512); ++attempt) {
        juce::Thread::sleep(1);
    }
    ASSERT_TRUE(source.ready(512));
    ASSERT_TRUE(worker.stopThread(1000));
    juce::AudioBuffer<float> block(2, 512);
    source.getNextAudioBlock({&block, 0, 512});
    EXPECT_GT(block.findMinMax(0, 0, 128).getStart(), 0.2f);
    EXPECT_FLOAT_EQ(block.getMagnitude(0, 128, 384), 0.0f);
    EXPECT_TRUE(source.ready(512));
    source.getNextAudioBlock({&block, 0, 512});
    EXPECT_FLOAT_EQ(block.getMagnitude(0, 0, 512), 0.0f);
    source.setNextReadPosition(64);
    EXPECT_TRUE(source.ready(512));
    source.getNextAudioBlock({&block, 0, 512});
    EXPECT_GT(block.findMinMax(0, 0, 64).getStart(), 0.2f);
    EXPECT_FLOAT_EQ(block.getMagnitude(0, 64, 448), 0.0f);
}

}  // namespace riffra
