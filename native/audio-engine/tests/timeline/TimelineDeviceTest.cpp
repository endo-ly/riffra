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
    pipeline.prepare(nullptr);
    auto snapshot = makeTestSnapshot();
    snapshot.graph.masterGainDb = -12.0;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(snapshot, formatManager, 48'000.0, 32, error, false))
        << error;

    // Act
    pipeline.setMasterGainDb(-3.0f);
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -3.0f);
    EXPECT_EQ(timeline.commitPreparedSnapshot(error), RealtimeRequest::accepted) << error;
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    pipeline.processBlock(nullptr, 0, outputs.data(), 2, static_cast<int>(left.size()), {});
    pipeline.deviceStopped();

    // Assert
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -12.0f);
}

TEST(AudioCommandDispatcherTest, QueueFullDoesNotPreventSafetyMutes) {
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
    std::shared_ptr<PluginEditorHost> auditionEditor;
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
        auditionEditor,
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
        EXPECT_NE(reply.getProperty("kind", {}).toString(), "error");
        ++requestId;
    }

    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::UserEmergency));
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::FeedbackProtection));
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::EngineTransition));
    EXPECT_TRUE(pipeline.isFeedbackSuspected());

    // Releasing a safety mute still requires its panic command to be queued.
    juce::var reply;
    dispatcher.dispatch(
        {requestId, SetEmergencyMuteCommand{false}},
        CommandResponder(requestId, [&reply](const juce::var& value) { reply = value; }));
    EXPECT_EQ(reply.getProperty("kind", {}).toString(), "error");
    const auto error = reply.getProperty("error", {});
    EXPECT_EQ(error.getProperty("kind", {}).toString(), "realtimeQueueFull");
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::UserEmergency));
}

TEST(AudioCommandDispatcherTest, PanicMustApplyBeforeASafetyMuteCanBeReleased) {
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
    std::shared_ptr<PluginEditorHost> auditionEditor;
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
        auditionEditor,
        comparisonRaw,
        comparisonProcessed,
        timelineOperationRunning,
    });
    timeline.setRealtimeOwner(RealtimeOwner::audio);
    auto requestId = std::uint64_t{1};
    const auto send = [&](const SidecarCommandSpec& command) {
        juce::var reply;
        dispatcher.dispatch(
            {requestId, command},
            CommandResponder(requestId, [&reply](const juce::var& value) { reply = value; }));
        ++requestId;
        return reply;
    };

    // Act
    EXPECT_NE(send(SetEmergencyMuteCommand{true}).getProperty("kind", {}).toString(), "error");
    const auto delayedRelease = send(SetEmergencyMuteCommand{false});

    // Assert
    EXPECT_EQ(delayedRelease.getProperty("kind", {}).toString(), "error");
    EXPECT_EQ(delayedRelease.getProperty("error", {}).getProperty("kind", {}).toString(),
              "timeout");
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::UserEmergency));

    timeline.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_NE(send(SetEmergencyMuteCommand{false}).getProperty("kind", {}).toString(), "error");
    EXPECT_FALSE(pipeline.hasMuteReason(MuteReason::UserEmergency));
}

TEST(AudioCommandDispatcherTest, StopWaitDoesNotBlockFurtherCommandDispatch) {
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
    std::shared_ptr<PluginEditorHost> auditionEditor;
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
        auditionEditor,
        comparisonRaw,
        comparisonProcessed,
        timelineOperationRunning,
    });
    timeline.setRealtimeOwner(RealtimeOwner::audio);
    juce::var stopReply;
    dispatcher.dispatch(
        {1, StopArrangeRecordingCommand{}},
        CommandResponder(1, [&stopReply](const juce::var& value) { stopReply = value; }));

    // Act
    juce::var statusReply;
    dispatcher.dispatch(
        {2, StatusCommand{}},
        CommandResponder(2, [&statusReply](const juce::var& value) { statusReply = value; }));

    // Assert
    EXPECT_FALSE(stopReply.isVoid());
    EXPECT_FALSE(statusReply.isVoid());
    timeline.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_TRUE(dispatcher.waitForBackgroundWork(std::chrono::seconds(2)));
}

}  // namespace riffra
