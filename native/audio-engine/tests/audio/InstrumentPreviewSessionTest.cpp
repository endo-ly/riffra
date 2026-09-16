#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <utility>

#include "audio/InstrumentPreviewSession.h"
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

TEST(InstrumentPreviewSessionTest, RendersPreparedBuiltInNoteEvents) {
    const auto preset = presetRoot().getChildFile("01-clean-sub-bass");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    juce::String error;
    auto session = InstrumentPreviewSession::create(
        definition.loadFileAsString(), preset.getFullPathName(), makeSpec(), 48'000.0, 256, error);
    ASSERT_NE(session, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    session->process(output.getArrayOfWritePointers(), output.getNumChannels(),
                     output.getNumSamples(), 48'000.0);

    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        for (int sample = 0; sample < output.getNumSamples(); ++sample)
            EXPECT_TRUE(std::isfinite(output.getSample(channel, sample)));
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_FALSE(session->hasFault());
}

TEST(PreviewEngineTest, BuiltInPreviewLeavesTakeComparisonVoiceIndependent) {
    const auto preset = presetRoot().getChildFile("01-clean-sub-bass");
    const auto definition = preset.getChildFile("definition.json");
    ASSERT_TRUE(definition.existsAsFile());
    PreviewEngine engine;
    juce::AudioBuffer<float> comparison(2, 32);
    comparison.clear();
    juce::String error;
    ASSERT_TRUE(
        engine.startPreview(comparison, 0, comparison.getNumSamples(), 1.0f, false, error, 1))
        << error.toStdString();
    ASSERT_TRUE(engine.startBuiltInPreview(definition.loadFileAsString(), preset.getFullPathName(),
                                           makeSpec(), 48'000.0, 256, error))
        << error.toStdString();

    engine.stopPreviewForKey(1);
    EXPECT_TRUE(engine.isPreviewing());

    engine.stopPreview();
    EXPECT_FALSE(engine.isPreviewing());
}

}  // namespace riffra
