#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <memory>
#include <thread>
#include <vector>

#include "app/AudioCommandDispatcher.h"
#include "device/AudioDeviceController.h"
#include "device/MidiInputService.h"
#include "plugins/PluginEditorHost.h"
#include "support/AudioRenderPipelineTestPeer.h"
#include "support/TimelineTestSupport.h"

namespace riffra {

class AudioCommandDispatcherTestPeer final {
public:
    static void setTrackPluginEditor(AudioCommandDispatcher& dispatcher,
                                     std::shared_ptr<PluginEditorHost> editor,
                                     EffectOwnerSpec owner, juce::String deviceId) {
        dispatcher.trackPluginEditor = std::move(editor);
        dispatcher.pluginEditorOwner = std::move(owner);
        dispatcher.trackPluginEditorDeviceId = std::move(deviceId);
        dispatcher.trackPluginEditorOpen.store(dispatcher.trackPluginEditor != nullptr,
                                               std::memory_order_release);
    }

    [[nodiscard]] static std::shared_ptr<PluginEditorHost> trackPluginEditor(
        const AudioCommandDispatcher& dispatcher) {
        return dispatcher.trackPluginEditor;
    }

    static void setTimelineOperationRunning(AudioCommandDispatcher& dispatcher,
                                            const bool running) {
        dispatcher.timelineOperationRunning.store(running, std::memory_order_release);
    }

    static void setRigEditor(AudioCommandDispatcher& dispatcher,
                             std::shared_ptr<PluginEditorHost> editor) {
        dispatcher.rigEditor = std::move(editor);
        dispatcher.rigEditorOpen.store(dispatcher.rigEditor != nullptr, std::memory_order_release);
    }

    [[nodiscard]] static std::shared_ptr<PluginEditorHost> rigEditor(
        const AudioCommandDispatcher& dispatcher) {
        return dispatcher.rigEditor;
    }
};

namespace {

std::vector<PluginDeviceSpec> rigEffects(const bool bypassed) {
    PluginStateSpec state;
    state.bypassed = bypassed;
    return {PluginDeviceSpec{"effect:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, std::move(state)}};
}

RigProjectionSpec rigProjection(std::vector<PluginDeviceSpec> effects,
                                const std::uint64_t revision) {
    return RigProjectionSpec{"rig-1", revision, 0, 0.0, std::move(effects)};
}

}  // namespace

TEST(AudioCommandDispatcherTest, CommitAcknowledgesAndRetainsDeferredEditorUntilIdle) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
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
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, lifecycle});
    AudioCommandDispatcherTestPeer::setTrackPluginEditor(dispatcher, editor, {},
                                                         juce::String("old-device"));
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
            dispatcher.dispatch({2, ClosePluginEditorCommand{100}},
                                CommandResponder(2, [&](const juce::var& reply) {
                                    firstWaitKind = reply.getProperty("kind", {}).toString();
                                }));
            rack->restoreAutomation();
            dispatcher.dispatch({3, ClosePluginEditorCommand{2000}},
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
    EXPECT_EQ(AudioCommandDispatcherTestPeer::trackPluginEditor(dispatcher), nullptr);
}

TEST(AudioCommandDispatcherTest, BypassingARigEffectKeepsTheInstanceAndItsEditor) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    RuntimeLifecycleExecutor lifecycle;
    AudioRenderPipelineTestPeer::prepare(pipeline, 48'000.0, 32);
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, lifecycle});
    std::uint64_t requestId = 0;
    std::atomic<bool> answered{false};
    std::shared_ptr<PluginRack> prepared;
    bool bypassKeptInstanceAndEditor = false;
    bool rebuildReleasedEditor = false;
    std::thread commands([&] {
        const auto send = [&](SidecarCommandSpec command) {
            const auto id = ++requestId;
            juce::String kind;
            juce::WaitableEvent done;
            dispatcher.dispatch({id, std::move(command)},
                                CommandResponder(id, [&](const juce::var& reply) {
                                    kind = reply.getProperty("kind", {}).toString();
                                    done.signal();
                                }));
            return done.wait(10'000) && kind == "response";
        };
        if (!send(OpenRigCommand{rigProjection(rigEffects(false), 1)})) return;
        prepared = pipeline.rig().findRack("effect:1");
        if (prepared == nullptr) return;
        AudioCommandDispatcherTestPeer::setRigEditor(dispatcher,
                                                     std::make_shared<PluginEditorHost>(*prepared));
        // A bypass change keeps the prepared chain and the editor it belongs to.
        if (!send(OpenRigCommand{rigProjection(rigEffects(true), 2)})) return;
        answered.store(true, std::memory_order_release);
        bypassKeptInstanceAndEditor =
            AudioCommandDispatcherTestPeer::rigEditor(dispatcher) != nullptr &&
            pipeline.rig().findRack("effect:1") == prepared &&
            pipeline.rig().findRack("effect:1")->status().bypassed;
        // A new chain replaces the instances, so the editor is closed.
        auto reordered = rigEffects(false);
        reordered.push_back(
            PluginDeviceSpec{"effect:2", RIFFRA_TEST_EFFECT_BUNDLE_PATH, PluginStateSpec{}});
        if (!send(OpenRigCommand{rigProjection(std::move(reordered), 3)})) return;
        rebuildReleasedEditor = AudioCommandDispatcherTestPeer::rigEditor(dispatcher) == nullptr;
        juce::MessageManager::callAsync(
            [] { juce::MessageManager::getInstance()->stopDispatchLoop(); });
    });
    juce::MessageManager::getInstance()->runDispatchLoop();
    commands.join();

    // Assert
    EXPECT_TRUE(answered.load());
    EXPECT_TRUE(bypassKeptInstanceAndEditor);
    EXPECT_TRUE(rebuildReleasedEditor);
}

TEST(AudioCommandDispatcherTest, ClosingWithoutAnEditorDoesNotWaitForARunningGraphLoad) {
    // Arrange
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    // The load task waits on these events, so they outlive the executor that runs it.
    juce::WaitableEvent loadStarted;
    juce::WaitableEvent releaseLoad;
    RuntimeLifecycleExecutor lifecycle;
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, lifecycle});
    ASSERT_TRUE(lifecycle.submitLoad([&] {
        loadStarted.signal();
        releaseLoad.wait(5000);
    }));
    ASSERT_TRUE(loadStarted.wait(2000));
    juce::String kind;

    // Act
    dispatcher.dispatch({1, ClosePluginEditorCommand{2000}},
                        CommandResponder(1, [&](const juce::var& reply) {
                            kind = reply.getProperty("kind", {}).toString();
                        }));
    const auto loadStillRunning = lifecycle.isBusy();
    releaseLoad.signal();

    // Assert
    EXPECT_EQ(kind, "response");
    EXPECT_TRUE(loadStillRunning);
}

TEST(AudioCommandDispatcherTest, QueueFullDoesNotPreventSafetyMutes) {
    // Arrange
    juce::AudioFormatManager formats;
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    AudioDeviceController deviceController(pipeline);
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, runtimeLifecycle});
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
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, runtimeLifecycle});
    AudioCommandDispatcherTestPeer::setTimelineOperationRunning(dispatcher, true);
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
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, runtimeLifecycle});
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
    MidiInputService midiInputs(pipeline.preview(), pipeline.audition(), timeline);
    RuntimeLifecycleExecutor runtimeLifecycle;
    AudioCommandDispatcher dispatcher(
        {formats, timeline, pipeline, deviceController, midiInputs, runtimeLifecycle});
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
    EXPECT_TRUE(pipeline.recording().waitForStopCompletion(std::chrono::seconds(2)));
}

}  // namespace riffra
