#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, RebuildsTimelineForTheCurrentAudioDeviceFormat) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::audioDeviceRestartRebuildsRuntimeFormat();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, KeepsUnavailableAutomationLanesAsGraphDiagnostics) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeInstrumentSnapshot("track:automation"), formats,
                                 48'000.0, 32, error))
        << error;

    ProcessorTrace trace;
    auto processor = std::make_unique<TestProcessor>(trace, true);
    processor->addParameter(new juce::AudioParameterFloat(
        juce::ParameterID{"test.manual", 1}, "Manual Parameter",
        juce::NormalisableRange<float>{0.0f, 1.0f}, 0.5f,
        juce::AudioParameterFloatAttributes().withAutomatable(false)));
    processor->addParameter(
        new juce::AudioParameterFloat(juce::ParameterID{"test.parameter", 1}, "Duplicate Parameter",
                                      juce::NormalisableRange<float>{0.0f, 1.0f}, 0.25f));
    auto rack = PluginRackTestPeer::installInstrument(std::move(processor), 48'000.0, 32, error);
    ASSERT_NE(rack, nullptr) << error;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(
        engine, "track:automation", "instrument:automation", std::move(rack)));

    auto candidate = makeInstrumentSnapshot("track:automation");
    candidate.revision = 2;
    candidate.graph.tracks.front().instrument =
        Vst3InstrumentSpec{"instrument:automation", "test-instrument.vst3", {}};
    candidate.graph.automation = {
        {AutomationTargetKind::deviceParameter,
         "instrument:automation",
         "test.manual",
         AutomationInterpolationSpec::linear,
         {{0, 0.5}}},
        {AutomationTargetKind::deviceParameter,
         "instrument:automation",
         "test.parameter",
         AutomationInterpolationSpec::linear,
         {{0, 0.5}}},
        {AutomationTargetKind::deviceParameter,
         "instrument:automation",
         "removed",
         AutomationInterpolationSpec::linear,
         {{0, 0.5}}},
    };

    // Act
    ASSERT_TRUE(engine.loadSnapshot(candidate, formats, 48'000.0, 32, error, false)) << error;

    // Assert
    auto* const pending = TimelineEngineTestPeer::pendingGraph(engine);
    ASSERT_NE(pending, nullptr);
    EXPECT_EQ(pending->summary.automationWarnings,
              (std::vector<juce::String>{
                  "instrument:automation/test.manual: automation parameter is not automatable "
                  "with this interpolation: test.manual",
                  "instrument:automation/test.parameter: automation parameter ID is ambiguous: "
                  "test.parameter",
                  "instrument:automation/removed: automation parameter ID is not present: removed",
              }));
    ASSERT_EQ(pending->tracks.size(), 1u);
    EXPECT_TRUE(pending->tracks.front()->runtime->deviceAutomation.empty());
}

}  // namespace riffra
