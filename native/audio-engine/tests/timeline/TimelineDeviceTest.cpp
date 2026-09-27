#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <memory>

#include "TimelineTestSupport.h"
#include "app/AudioCommandDispatcher.h"
#include "device/AudioDeviceController.h"
#include "midi/MidiInputService.h"

namespace riffra {

TEST(TimelineEngineTest, KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused) {
    EXPECT_TRUE(TimelineEngineTestPeer::canonicalTrackStateSurvivesReusableDeviceCommit());
}

TEST(TimelineEngineTest, AppliesEditorParameterToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::editorParameterUpdatesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, AppliesPluginStateToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::persistedStateUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, AppliesPluginProgramToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::programChangeUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, SendsEmergencyPanicToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::panicClosesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, GraphCommitRestoresCanonicalMasterGainAfterPreview) {
    // Arrange
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    std::shared_ptr<PluginEditorHost> trackPluginEditor;
    juce::String trackPluginEditorTrackId;
    juce::String trackPluginEditorDeviceId;
    juce::AudioBuffer<float> comparisonRaw;
    juce::AudioBuffer<float> comparisonProcessed;
    std::atomic<bool> timelineOperationRunning{false};
    AudioCommandDispatcher dispatcher({
        formatManager,
        timeline,
        pipeline,
        deviceController,
        midiInputs,
        runtimeLifecycle,
        trackPluginEditor,
        trackPluginEditorTrackId,
        trackPluginEditorDeviceId,
        comparisonRaw,
        comparisonProcessed,
        timelineOperationRunning,
    });
    auto snapshot = makeTestSnapshot();
    snapshot.graph.masterGainDb = -12.0;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(snapshot, formatManager, 48'000.0, 32, error, false))
        << error;
    auto* preview = new juce::DynamicObject();
    preview->setProperty("type", "previewMasterGainDb");
    preview->setProperty("gainDb", -3.0);

    // Act
    const auto previewResult = dispatcher.dispatch(juce::var(preview));
    juce::ignoreUnused(previewResult);
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -3.0f);
    auto* commit = new juce::DynamicObject();
    commit->setProperty("type", "commitTimelineSnapshot");
    const auto commitResult = dispatcher.dispatch(juce::var(commit));
    juce::ignoreUnused(commitResult);
    ASSERT_TRUE(runtimeLifecycle.waitForIdle(std::chrono::seconds(5)));

    // Assert
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -12.0f);
}

}  // namespace riffra
