#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <utility>

#include "../timeline/SonalloyTestSupport.h"
#include "audio/InstrumentPreviewSession.h"
#include "audio/InstrumentPreviewTiming.h"
#include "audio/PreviewEngine.h"

namespace riffra {
namespace {

juce::File presetRoot() { return juce::File(RIFFRA_SONALLOY_TEST_PRESET_ROOT); }

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

TEST(InstrumentPreviewSessionTest, RejectsInvalidPreviewNotesBeforeRuntimeCreation) {
    auto spec = makeSpec();
    spec.notes.front().velocity = 0;
    juce::String error;

    const auto session = InstrumentPreviewSession::create("{}", presetRoot().getFullPathName(),
                                                          std::move(spec), 48'000.0, 256, error);

    EXPECT_EQ(session, nullptr);
    EXPECT_FALSE(error.isEmpty());
}

TEST(InstrumentPreviewSessionTest, CalculatesBarPositionForCompoundMeters) {
    const auto barPosition = instrument_preview::barPositionForFrame(72'000, 48'000.0, 120.0, 6, 8);

    EXPECT_NEAR(barPosition, 1.0, 1.0e-12);
}

TEST(InstrumentPreviewSessionTest, RendersPreparedInstrumentNoteEvents) {
    const auto preset = test::builtInPresetDirectory("BASS-001");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    juce::String error;
    auto session = InstrumentPreviewSession::create(
        definition.loadFileAsString(), preset.getFullPathName(), makeSpec(), 48'000.0, 256, error);
    ASSERT_NE(session, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    session->process(output.getArrayOfWritePointers(), output.getNumChannels(),
                     output.getNumSamples(), 48'000.0);

    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        for (int sample = 0; sample < output.getNumSamples(); ++sample)
            EXPECT_TRUE(std::isfinite(output.getSample(channel, sample)));
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_FALSE(session->hasFault());
}

TEST(InstrumentPreviewSessionTest, AddsPreviewToAnExistingFiniteDestinationSignal) {
    const auto preset = test::builtInPresetDirectory("BASS-001");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());

    juce::String previewError;
    auto previewSession =
        InstrumentPreviewSession::create(definition.loadFileAsString(), preset.getFullPathName(),
                                         makeSpec(), 48'000.0, 256, previewError);
    ASSERT_NE(previewSession, nullptr) << previewError.toStdString();

    juce::AudioBuffer<float> previewOnly(2, 256);
    previewOnly.clear();
    previewSession->process(previewOnly.getArrayOfWritePointers(), previewOnly.getNumChannels(),
                            previewOnly.getNumSamples(), 48'000.0);

    juce::String mixedError;
    auto mixedSession =
        InstrumentPreviewSession::create(definition.loadFileAsString(), preset.getFullPathName(),
                                         makeSpec(), 48'000.0, 256, mixedError);
    ASSERT_NE(mixedSession, nullptr) << mixedError.toStdString();

    constexpr float existingSignal = 0.25f;
    juce::AudioBuffer<float> mixed(2, 256);
    mixed.clear();
    for (int channel = 0; channel < mixed.getNumChannels(); ++channel)
        for (int sample = 0; sample < mixed.getNumSamples(); ++sample)
            mixed.setSample(channel, sample, existingSignal);
    mixedSession->process(mixed.getArrayOfWritePointers(), mixed.getNumChannels(),
                          mixed.getNumSamples(), 48'000.0);

    for (int channel = 0; channel < mixed.getNumChannels(); ++channel)
        for (int sample = 0; sample < mixed.getNumSamples(); ++sample) {
            EXPECT_TRUE(std::isfinite(mixed.getSample(channel, sample)));
            EXPECT_NEAR(mixed.getSample(channel, sample) - previewOnly.getSample(channel, sample),
                        existingSignal, 1.0e-6f);
        }
}

TEST(InstrumentPreviewSessionTest, RejectsInvalidPreviewNumeratorAndNoteOrder) {
    auto spec = makeSpec();
    spec.timeSignature.numerator = 33;
    juce::String error;

    auto session = InstrumentPreviewSession::create("{}", presetRoot().getFullPathName(),
                                                    std::move(spec), 48'000.0, 256, error);

    EXPECT_EQ(session, nullptr);
    EXPECT_FALSE(error.isEmpty());

    spec = makeSpec();
    spec.notes.push_back(InstrumentPreviewNote{120, 120, 40, 100});
    std::swap(spec.notes[0], spec.notes[1]);
    error.clear();
    session = InstrumentPreviewSession::create("{}", presetRoot().getFullPathName(),
                                               std::move(spec), 48'000.0, 256, error);

    EXPECT_EQ(session, nullptr);
    EXPECT_FALSE(error.isEmpty());
}

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
    EXPECT_TRUE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());

    juce::AudioBuffer<float> output(2, 256);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_GT(maximumMagnitude(output), 0.0f);

    engine.stopInstrumentPreview();

    EXPECT_FALSE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    output.clear();
    ASSERT_TRUE(engine.tryMix(output.getArrayOfWritePointers(), output.getNumChannels(),
                              output.getNumSamples(), 48'000.0));
    EXPECT_FLOAT_EQ(maximumMagnitude(output), 0.0f);
    engine.stopPreview();
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
    EXPECT_TRUE(engine.isInstrumentPreviewing());

    engine.stopPreviewForKey(1);
    EXPECT_TRUE(engine.isInstrumentPreviewing());
    EXPECT_TRUE(engine.isPreviewing());

    engine.stopPreview();
    EXPECT_FALSE(engine.isPreviewing());
}

}  // namespace riffra
