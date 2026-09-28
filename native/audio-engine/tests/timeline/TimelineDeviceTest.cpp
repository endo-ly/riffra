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

TEST(TimelineEngineTest, WarmsUpAndResetsPluginDevices) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::pluginDevicesWarmUpAndReset();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, SendsEmergencyPanicToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::panicClosesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, GraphPublicationRestoresCanonicalMasterGainAfterPreview) {
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
    pipeline.prepare(nullptr);
    auto snapshot = makeTestSnapshot();
    snapshot.graph.masterGainDb = -12.0;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(snapshot, formatManager, 48'000.0, 32, error, false))
        << error;
    const auto discard = [](const juce::var&) {};

    // Act
    dispatcher.dispatch({1, PreviewMasterGainDbCommand{-3.0}}, CommandResponder(1, discard));
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -3.0f);
    dispatcher.dispatch({2, CommitTimelineSnapshotCommand{}}, CommandResponder(2, discard));
    ASSERT_TRUE(runtimeLifecycle.waitForIdle(std::chrono::seconds(5)));
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    pipeline.processBlock(nullptr, 0, outputs.data(), 2, static_cast<int>(left.size()), {});

    // Assert
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -12.0f);
}

TEST(AudioCommandDispatcherTest, QueueFullLeavesMuteStatesUnchanged) {
    // Arrange
    juce::AudioFormatManager formats;
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
        formats,
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
    timeline.setRealtimeOwner(RealtimeOwner::audio);
    for (std::size_t command = 0; command < TimelineEngineTestPeer::realtimeCommandCapacity();
         ++command)
        ASSERT_TRUE(timeline.play());

    const std::array<SidecarCommandSpec, 3> muteCommands{
        SetEmergencyMuteCommand{true},
        SetFeedbackProtectionCommand{true},
        SetEngineTransitionMuteCommand{true},
    };
    auto requestId = std::uint64_t{1};

    // Act
    for (const auto& command : muteCommands) {
        juce::var reply;
        dispatcher.dispatch(
            {requestId, command},
            CommandResponder(requestId, [&reply](const juce::var& value) { reply = value; }));

        // Assert
        EXPECT_EQ(reply.getProperty("kind", {}).toString(), "error");
        const auto error = reply.getProperty("error", {});
        EXPECT_EQ(error.getProperty("kind", {}).toString(), "realtimeQueueFull");
        ++requestId;
    }

    EXPECT_FALSE(pipeline.hasMuteReason(MuteReason::UserEmergency));
    EXPECT_FALSE(pipeline.hasMuteReason(MuteReason::FeedbackProtection));
    EXPECT_FALSE(pipeline.hasMuteReason(MuteReason::EngineTransition));
    EXPECT_FALSE(pipeline.isFeedbackSuspected());
}

}  // namespace riffra
