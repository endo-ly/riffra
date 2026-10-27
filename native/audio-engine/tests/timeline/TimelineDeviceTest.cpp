#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <memory>
#include <optional>
#include <vector>

#include "TimelineTestSupport.h"
#include "app/AudioCommandDispatcher.h"
#include "device/AudioDeviceController.h"
#include "midi/MidiInputService.h"
#include "plugins/PluginEditorHost.h"

namespace riffra {

TEST(AudioCommandDispatcherTest, CommitAcknowledgesAndRetainsDeferredEditorUntilIdle) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), timeline);
    RuntimeLifecycleExecutor lifecycle;
    ProcessorTrace trace;
    juce::String error;
    auto owned = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace, true, true),
                                             48'000.0, 32, error);
    ASSERT_NE(owned, nullptr) << error;
    std::shared_ptr<PluginRack> rack(std::move(owned));
    std::atomic<bool> captured{false};
    auto editor = std::make_shared<PluginEditorHost>(
        *rack, PluginEditorHost::StateCallback{},
        [&](const auto& state, const auto&) { captured.store(state.has_value()); },
        PluginEditorHost::ParameterCallback{}, PluginEditorHost::ClosedCallback{}, rack);
    ASSERT_TRUE(editor->open(error)) << error;
    rack->beginAutomationRead();
    auto trackEditor = editor;
    EffectOwnerSpec editorOwner;
    juce::String editorDeviceId = "old-device";
    std::shared_ptr<PluginEditorHost> auditionEditor;
    juce::AudioBuffer<float> raw, processed;
    std::atomic<bool> operationRunning{false};
    AudioCommandDispatcher dispatcher({formats, timeline, pipeline, deviceController, midiInputs,
                                       lifecycle, trackEditor, editorOwner, editorDeviceId,
                                       auditionEditor, raw, processed, operationRunning});
    ASSERT_TRUE(timeline.loadSnapshot(makeTestSnapshot(), formats, 48'000.0, 32, error, false))
        << error;

    juce::WaitableEvent acknowledged;
    juce::String commitKind, firstWaitKind, secondWaitKind;
    std::thread commands([&] {
        dispatcher.dispatch({1, CommitTimelineSnapshotCommand{}},
                            CommandResponder(1, [&](const juce::var& reply) {
                                commitKind = reply.getProperty("kind", {}).toString();
                                acknowledged.signal();
                            }));
        if (acknowledged.wait(2000) && lifecycle.waitForIdle(std::chrono::seconds(2))) {
            dispatcher.dispatch({2, WaitForTimelineIdleCommand{100, true}},
                                CommandResponder(2, [&](const juce::var& reply) {
                                    firstWaitKind = reply.getProperty("kind", {}).toString();
                                }));
            rack->restoreAutomation();
            dispatcher.dispatch({3, WaitForTimelineIdleCommand{2000, true}},
                                CommandResponder(3, [&](const juce::var& reply) {
                                    secondWaitKind = reply.getProperty("kind", {}).toString();
                                }));
        }
        juce::MessageManager::callAsync(
            [] { juce::MessageManager::getInstance()->stopDispatchLoop(); });
    });
    juce::MessageManager::getInstance()->runDispatchLoop();
    commands.join();

    EXPECT_EQ(commitKind, "response");
    EXPECT_EQ(firstWaitKind, "error");
    EXPECT_EQ(secondWaitKind, "response");
    EXPECT_TRUE(captured.load());
    EXPECT_EQ(trackEditor, nullptr);
}

TEST(TimelineEngineTest, KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused) {
    EXPECT_TRUE(TimelineEngineTestPeer::canonicalTrackStateSurvivesReusableDeviceCommit());

    const PluginDeviceSpec original{
        "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.1f, 0.2f}, false}};
    const PluginDeviceSpec edited{
        "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.8f, 0.9f}, true}};
    EXPECT_TRUE(sameEffectTopology(std::vector<PluginDeviceSpec>{original},
                                   std::vector<PluginDeviceSpec>{edited}));
}

TEST(TimelineEngineTest, AppliesEditorEditsToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto parameter = TimelineEngineTestPeer::editorParameterUpdatesInstrumentRuntime();
    const auto state = TimelineEngineTestPeer::persistedStateUpdatesInstrumentRuntime();
    const auto program = TimelineEngineTestPeer::programChangeUpdatesInstrumentRuntime();

    // Assert: a parameter, a persisted state, and a program all reach the runtime.
    EXPECT_TRUE(parameter);
    EXPECT_TRUE(state);
    EXPECT_TRUE(program);
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
    EffectOwnerSpec pluginEditorOwner;
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
        pluginEditorOwner,
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

    const std::array<SidecarCommandSpec, 3> releaseCommands{
        SetEmergencyMuteCommand{false},
        SetFeedbackProtectionCommand{false},
        SetEngineTransitionMuteCommand{false},
    };
    const std::array<juce::String, 3> operations{"audio.emergencyMute", "audio.feedbackProtection",
                                                 "audio.engineTransitionMute"};
    for (std::size_t index = 0; index < releaseCommands.size(); ++index) {
        juce::var reply;
        dispatcher.dispatch(
            {requestId, releaseCommands[index]},
            CommandResponder(requestId, [&reply](const juce::var& value) { reply = value; }));
        EXPECT_EQ(reply.getProperty("kind", {}).toString(), "error");
        const auto error = reply.getProperty("error", {});
        EXPECT_EQ(error.getProperty("kind", {}).toString(), "realtimeQueueFull");
        EXPECT_EQ(error.getProperty("operation", {}).toString(), operations[index]);
        ++requestId;
    }
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::UserEmergency));
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::FeedbackProtection));
    EXPECT_TRUE(pipeline.hasMuteReason(MuteReason::EngineTransition));
}

TEST(AudioCommandDispatcherTest, RejectsDeviceChangesDuringGraphOperations) {
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    std::shared_ptr<PluginEditorHost> trackPluginEditor;
    EffectOwnerSpec pluginEditorOwner;
    juce::String trackPluginEditorDeviceId;
    std::shared_ptr<PluginEditorHost> auditionEditor;
    juce::AudioBuffer<float> comparisonRaw;
    juce::AudioBuffer<float> comparisonProcessed;
    std::atomic<bool> timelineOperationRunning{true};
    AudioCommandDispatcher dispatcher({
        formats,
        timeline,
        pipeline,
        deviceController,
        midiInputs,
        runtimeLifecycle,
        trackPluginEditor,
        pluginEditorOwner,
        trackPluginEditorDeviceId,
        auditionEditor,
        comparisonRaw,
        comparisonProcessed,
        timelineOperationRunning,
    });
    juce::var reply;

    dispatcher.dispatch(
        {1,
         SetDeviceParameterCommand{
             {EffectOwnerKindSpec::track, "track:1"}, "device:1", 0, 0.75f, std::nullopt}},
        CommandResponder(1, [&reply](const juce::var& value) { reply = value; }));

    ASSERT_EQ(reply.getProperty("kind", {}).toString(), "error");
    EXPECT_EQ(reply.getProperty("error", {}).getProperty("kind", {}).toString(), "timelineBusy");
    EXPECT_TRUE(runtimeLifecycle.waitForIdle(std::chrono::milliseconds(100)));
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
    EffectOwnerSpec pluginEditorOwner;
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
        pluginEditorOwner,
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

    const std::array<SidecarCommandSpec, 3> muteCommands{
        SetEmergencyMuteCommand{true},
        SetFeedbackProtectionCommand{true},
        SetEngineTransitionMuteCommand{true},
    };
    const std::array<SidecarCommandSpec, 3> releaseCommands{
        SetEmergencyMuteCommand{false},
        SetFeedbackProtectionCommand{false},
        SetEngineTransitionMuteCommand{false},
    };
    const std::array<MuteReason, 3> reasons{
        MuteReason::UserEmergency, MuteReason::FeedbackProtection, MuteReason::EngineTransition};
    const std::array<juce::String, 3> operations{"audio.emergencyMute", "audio.feedbackProtection",
                                                 "audio.engineTransitionMute"};

    // Act / Assert
    for (const auto& command : muteCommands)
        EXPECT_NE(send(command).getProperty("kind", {}).toString(), "error");
    for (std::size_t index = 0; index < releaseCommands.size(); ++index) {
        const auto delayedRelease = send(releaseCommands[index]);
        EXPECT_EQ(delayedRelease.getProperty("kind", {}).toString(), "error");
        const auto error = delayedRelease.getProperty("error", {});
        EXPECT_EQ(error.getProperty("kind", {}).toString(), "timeout");
        EXPECT_EQ(error.getProperty("operation", {}).toString(), operations[index]);
        EXPECT_TRUE(pipeline.hasMuteReason(reasons[index]));
    }

    timeline.setRealtimeOwner(RealtimeOwner::control);
    for (std::size_t index = 0; index < releaseCommands.size(); ++index) {
        EXPECT_NE(send(releaseCommands[index]).getProperty("kind", {}).toString(), "error");
        EXPECT_FALSE(pipeline.hasMuteReason(reasons[index]));
        for (std::size_t remaining = index + 1; remaining < reasons.size(); ++remaining)
            EXPECT_TRUE(pipeline.hasMuteReason(reasons[remaining]));
    }
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
    EffectOwnerSpec pluginEditorOwner;
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
        pluginEditorOwner,
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
