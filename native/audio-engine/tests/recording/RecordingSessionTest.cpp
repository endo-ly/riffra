#include <gtest/gtest.h>

#include <array>

#include "../support/TestSupport.h"
#include "recording/RecordingSession.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 44'100.0;
constexpr int kChannels = 2;
constexpr int kBlockSize = 128;
constexpr int kTotalSamples = 2'048;

bool writeSyntheticTake(RecordingSession& session) {
    std::array<std::array<float, kBlockSize>, kChannels> rawBuffers{};
    std::array<std::array<float, kBlockSize>, kChannels> processedBuffers{};
    std::array<const float*, kChannels> rawPointers{};
    std::array<const float*, kChannels> processedPointers{};
    for (std::size_t channel = 0; channel < kChannels; ++channel) {
        const auto value = channel == 0 ? 0.25f : -0.25f;
        rawBuffers[channel].fill(value);
        processedBuffers[channel].fill(value * 0.5f);
        rawPointers[channel] = rawBuffers[channel].data();
        processedPointers[channel] = processedBuffers[channel].data();
    }

    for (int offset = 0; offset < kTotalSamples; offset += kBlockSize) {
        const auto count = juce::jmin(kBlockSize, kTotalSamples - offset);
        if (!session.writeRaw(rawPointers.data(), count)) return false;
    }
    session.flushRaw();
    for (int offset = 0; offset < kTotalSamples; offset += kBlockSize) {
        const auto count = juce::jmin(kBlockSize, kTotalSamples - offset);
        if (!session.writeProcessedOffline(processedPointers.data(), count, 1'000)) return false;
    }
    return true;
}

class RecordingSessionTest : public testing::Test {
protected:
    test::TemporaryDirectory directory;
};

}  // namespace

TEST_F(RecordingSessionTest, FinalizesRawAndProcessedWaveFilesWithAManifest) {
    juce::String error;
    auto session =
        RecordingSession::create(directory.get(), kSampleRate, kChannels, kChannels, error);
    ASSERT_NE(session, nullptr) << error;
    ASSERT_TRUE(writeSyntheticTake(*session));
    ASSERT_TRUE(session->finish(true, error)) << error;

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    std::unique_ptr<juce::AudioFormatReader> rawReader(
        formats.createReaderFor(directory.get().getChildFile("raw.wav")));
    std::unique_ptr<juce::AudioFormatReader> processedReader(
        formats.createReaderFor(directory.get().getChildFile("processed.wav")));
    ASSERT_NE(rawReader, nullptr);
    ASSERT_NE(processedReader, nullptr);
    EXPECT_EQ(rawReader->numChannels, kChannels);
    EXPECT_EQ(processedReader->numChannels, kChannels);
    EXPECT_EQ(rawReader->lengthInSamples, kTotalSamples);
    EXPECT_EQ(processedReader->lengthInSamples, kTotalSamples);

    const auto manifest = test::parseJsonFile(directory.get().getChildFile("manifest.json"));
    ASSERT_TRUE(manifest.isObject());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "completed");
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("samplesWritten", -1)), kTotalSamples);
    EXPECT_EQ(manifest.getProperty("rawFile", {}).toString(), "raw.wav");
    EXPECT_EQ(manifest.getProperty("processedFile", {}).toString(), "processed.wav");
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("missingSamples", -1)), 0);
    EXPECT_EQ(manifest.getProperty("recoveryStatus", {}).toString(), "clean");
}

TEST_F(RecordingSessionTest, PreservesIncompleteRecordingAsRecoverable) {
    const auto incompleteDirectory = directory.get().getChildFile("incomplete");
    juce::String error;
    auto session =
        RecordingSession::create(incompleteDirectory, kSampleRate, kChannels, kChannels, error);
    ASSERT_NE(session, nullptr) << error;

    EXPECT_FALSE(session->finish(true, error));
    const auto manifest = test::parseJsonFile(incompleteDirectory.getChildFile("manifest.json"));
    ASSERT_TRUE(manifest.isObject());
    EXPECT_EQ(manifest.getProperty("state", {}).toString(), "recoverable");
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("samplesWritten", -1)), 0);
    EXPECT_EQ(static_cast<juce::int64>(manifest.getProperty("missingSamples", -1)), 0);
    EXPECT_TRUE(incompleteDirectory.getChildFile("raw.wav.partial").existsAsFile());
    EXPECT_TRUE(incompleteDirectory.getChildFile("processed.wav.partial").existsAsFile());
}

}  // namespace riffra
