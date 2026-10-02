#include <gtest/gtest.h>

#include "audio/PreviewEngine.h"

namespace riffra {

class PreviewEngineTestPeer final {
public:
    static std::size_t bufferCount(PreviewEngine& engine) {
        const std::lock_guard lock(engine.controlMutex);
        engine.reclaimRetiredState();
        return engine.previewBuffers.size();
    }
};

TEST(PreviewEngineTest, RejectsAFullCommandQueueAndReclaimsOnlyFinishedVoices) {
    // Arrange
    PreviewEngine engine;
    juce::AudioBuffer<float> source(2, 512);
    for (int channel = 0; channel < 2; ++channel)
        juce::FloatVectorOperations::fill(source.getWritePointer(channel), 0.5f, 512);
    juce::String error;
    for (int update = 0; update < 256; ++update)
        ASSERT_TRUE(engine.startPreview(source, 0, 512, 1.0f, true, error, 1)) << error;

    // Act
    EXPECT_FALSE(engine.startPreview(source, 0, 512, 1.0f, true, error, 1));
    juce::String stopError;
    EXPECT_FALSE(engine.stopPreview(&stopError));
    EXPECT_TRUE(stopError.contains("queue is full"));
    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    const auto playingBuffers = PreviewEngineTestPeer::bufferCount(engine);
    engine.stopPreview();
    const auto fadingBuffers = PreviewEngineTestPeer::bufferCount(engine);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    const auto retiredBuffers = PreviewEngineTestPeer::bufferCount(engine);

    // Assert
    EXPECT_TRUE(error.contains("queue is full"));
    EXPECT_EQ(playingBuffers, 1u);
    EXPECT_EQ(fadingBuffers, 1u);
    EXPECT_GT(output.getMagnitude(0, 0, 256), 0.0f);
    EXPECT_EQ(retiredBuffers, 0u);
    EXPECT_FALSE(engine.isPreviewing());
}

TEST(PreviewEngineTest, StopsSampleAndSynthWithOneRemainingCommandSlot) {
    // Arrange
    PreviewEngine engine;
    juce::AudioBuffer<float> source(2, 512);
    source.clear();
    juce::String error;
    ASSERT_TRUE(engine.startPreview(source, 0, 512, 1.0f, true, error));
    ASSERT_TRUE(engine.startSynthNote(60, 0.1f));
    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0);
    ASSERT_TRUE(engine.isPreviewing());
    for (int command = 0; command < 255; ++command) ASSERT_TRUE(engine.startSynthNote(60, 0.1f));

    // Act
    ASSERT_TRUE(engine.stopPreview(&error)) << error;
    EXPECT_FALSE(engine.stopPreview(&error));
    output.clear();
    engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0);

    // Assert
    EXPECT_FALSE(engine.isPreviewing());
    EXPECT_EQ(PreviewEngineTestPeer::bufferCount(engine), 0u);
}

}  // namespace riffra
