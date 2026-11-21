#include <gtest/gtest.h>

#include <algorithm>

#include "pipeline/AudioRenderPipeline.h"
#include "preview/InstrumentPreviewSession.h"
#include "preview/PreviewEngine.h"
#include "support/SonalloyTestSupport.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

class PreviewEngineTestPeer final {
public:
    static std::size_t resourceCount(PreviewEngine& engine) {
        const std::lock_guard lock(engine.controlMutex);
        engine.reclaimRetiredState();
        return engine.previewBuffers.size() + engine.instrumentSessions.size() +
               engine.instrumentBuffers.size();
    }
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

TEST(PreviewEngineTest, StopsAudibleVoicesWhileMutedWithoutReplayingTheirFade) {
    const std::array setters{
        &AudioRenderPipeline::setUserEmergencyMute, &AudioRenderPipeline::setEngineTransitionMute,
        &AudioRenderPipeline::setDeviceFaulted, &AudioRenderPipeline::setFeedbackProtection};
    for (const auto setter : setters) {
        for (int sourceKind = 0; sourceKind < 3; ++sourceKind) {
            // Arrange
            TimelineEngine timeline;
            AudioRenderPipeline pipeline(timeline);
            auto& preview = pipeline.preview();
            juce::AudioBuffer<float> source(2, 1024);
            for (int channel = 0; channel < 2; ++channel)
                juce::FloatVectorOperations::fill(source.getWritePointer(channel), 0.5f, 1024);
            juce::String error;
            if (sourceKind == 0) {
                ASSERT_TRUE(preview.startPreview(source, 0, 1024, 1.0f, true, error));
            } else if (sourceKind == 1) {
                ASSERT_TRUE(preview.startSynthNote(60, 0.1f));
            } else {
                const auto preset = test::builtInPresetDirectory("BASS-001");
                InstrumentPreviewSpec spec;
                spec.tempoBpm = 120.0;
                spec.ticksPerBeat = 480;
                spec.timeSignature = {4, 4};
                spec.lengthTicks = 480;
                spec.notes.push_back({0, 240, 36, 100});
                ASSERT_TRUE(preview.startInstrumentPreview(
                    preset.getChildFile("definition.json").loadFileAsString(),
                    preset.getFullPathName(), spec, 48'000.0, 256, error))
                    << error;
            }
            juce::AudioBuffer<float> output(2, 256);
            output.clear();
            preview.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0);
            ASSERT_GT(output.getMagnitude(0, 0, 256), 0.0f) << sourceKind;
            ASSERT_TRUE(preview.isPreviewing());

            // Act
            (pipeline.*setter)(true);
            ASSERT_TRUE(pipeline.preview().stopPreview(&error)) << error;
            pipeline.processBlock(nullptr, 0, output.getArrayOfWritePointers(), 2, 256, {});
            const auto stoppedWhileMuted = !preview.isPreviewing();
            const auto retainedResources = PreviewEngineTestPeer::resourceCount(preview);
            (pipeline.*setter)(false);
            output.clear();
            preview.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0);

            // Assert
            EXPECT_TRUE(stoppedWhileMuted) << sourceKind;
            EXPECT_EQ(retainedResources, 0u) << sourceKind;
            EXPECT_FLOAT_EQ(output.getMagnitude(0, 0, 256), 0.0f) << sourceKind;
            EXPECT_FALSE(preview.isInstrumentPreviewing());
            EXPECT_FALSE(preview.isPreviewing());
        }
    }
}

namespace {

InstrumentPreviewSpec makeSpec() {
    InstrumentPreviewSpec spec;
    spec.tempoBpm = 120.0;
    spec.ticksPerBeat = 480;
    spec.timeSignature = InstrumentPreviewTimeSignature{4, 4};
    spec.lengthTicks = 480;
    spec.notes.push_back(InstrumentPreviewNote{0, 240, 36, 100});
    return spec;
}

float maximumMagnitude(const juce::AudioBuffer<float>& output) {
    float magnitude = 0.0f;
    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        magnitude = std::max(magnitude, output.getMagnitude(channel, 0, output.getNumSamples()));
    return magnitude;
}

}  // namespace

TEST(PreviewEngineTest, StopsInstrumentPreviewWithoutStoppingTakeComparison) {
    const auto preset = test::builtInPresetDirectory("BASS-001");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    PreviewEngine engine;
    juce::AudioBuffer<float> comparison(2, 32);
    comparison.clear();
    juce::String error;
    ASSERT_TRUE(
        engine.startPreview(comparison, 0, comparison.getNumSamples(), 1.0f, true, error, 1))
        << error.toStdString();
    ASSERT_TRUE(engine.startInstrumentPreview(
        definition.loadFileAsString(), preset.getFullPathName(), makeSpec(), 48'000.0, 256, error))
        << error.toStdString();
    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_TRUE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());

    engine.stopInstrumentPreview();

    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_FALSE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_FLOAT_EQ(maximumMagnitude(output), 0.0f);
    engine.stopPreview();
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    EXPECT_FALSE(engine.isPreviewing());
}

TEST(PreviewEngineTest, NaturalInstrumentPreviewFinishLeavesTakeComparisonActive) {
    const auto preset = test::builtInPresetDirectory("BASS-001");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    PreviewEngine engine;
    juce::AudioBuffer<float> comparison(2, 32);
    comparison.clear();
    juce::String error;
    ASSERT_TRUE(
        engine.startPreview(comparison, 0, comparison.getNumSamples(), 1.0f, true, error, 1))
        << error.toStdString();
    ASSERT_TRUE(engine.startInstrumentPreview(
        definition.loadFileAsString(), preset.getFullPathName(), makeSpec(), 48'000.0, 256, error))
        << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    for (int block = 0; block < 4096 && engine.isInstrumentPreviewing(); ++block) {
        output.clear();
        ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                                  output.getNumSamples(), 48'000.0));
    }

    EXPECT_FALSE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());
    engine.stopPreview();
}

TEST(PreviewEngineTest, InstrumentPreviewLeavesTakeComparisonVoiceIndependent) {
    const auto preset = test::builtInPresetDirectory("BASS-001");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    PreviewEngine engine;
    juce::AudioBuffer<float> comparison(2, 32);
    comparison.clear();
    juce::String error;
    ASSERT_TRUE(
        engine.startPreview(comparison, 0, comparison.getNumSamples(), 1.0f, false, error, 1))
        << error.toStdString();
    ASSERT_TRUE(engine.startInstrumentPreview(
        definition.loadFileAsString(), preset.getFullPathName(), makeSpec(), 48'000.0, 256, error))
        << error.toStdString();
    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    EXPECT_TRUE(engine.isInstrumentPreviewing());

    engine.stopPreviewForKey(1);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    EXPECT_TRUE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());

    engine.stopPreview();
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), 2, 256, 48'000.0));
    EXPECT_FALSE(engine.isPreviewing());
}

}  // namespace riffra
