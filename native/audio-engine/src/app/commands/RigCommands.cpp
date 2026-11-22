#include <algorithm>
#include <chrono>
#include <climits>
#include <cstdint>
#include <memory>

#include "app/AudioCommandDispatcher.h"
#include "plugins/PluginEditorHost.h"
#include "protocol/AudioProtocol.h"

namespace riffra {
namespace {

constexpr auto kRigLoadWatchdogTimeout = std::chrono::seconds(45);
constexpr auto kRigCommandQueueTimeout = std::chrono::seconds(8);
constexpr auto kRigEditorWatchdogTimeout = std::chrono::seconds(30);
constexpr auto kRigCloseWatchdogTimeout = std::chrono::seconds(10);
/// Budget for closing the rig editor and taking its closing state.
constexpr auto kRigEditorCloseTimeout = std::chrono::seconds(5);

void failExpiredRigCommand(const std::shared_ptr<CommandResponder>& pending,
                           const juce::String& operation) {
    pending->fail("runtimeLifecycle", "Rig command expired before execution.", operation);
}

}  // namespace

void AudioCommandDispatcher::handle(const OpenRigCommand& command, CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still loading a VST3. The rig can be started when it "
            "finishes."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submitWithQueueDeadline(
        [this, projection = command.projection, pending] {
            juce::String error;
            bool opened = false;
            try {
                opened = openRig(projection, error);
            } catch (const std::exception& exception) {
                error = "Starting the rig raised an exception: " + juce::String(exception.what());
            } catch (...) {
                error = "Starting the rig failed with an unknown exception.";
            }
            timelineOperationRunning.store(false, std::memory_order_release);
            if (!opened) {
                pending->fail("rig", error, "rig.start");
                return;
            }
            pending->respond(RigAckSpec{});
        },
        kRigLoadWatchdogTimeout, kRigCommandQueueTimeout,
        [this, pending] {
            timelineOperationRunning.store(false, std::memory_order_release);
            failExpiredRigCommand(pending, "rig.start");
        });
    if (!submitted) {
        timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.", "rig.start");
    }
}

void AudioCommandDispatcher::handle(const CloseRigCommand&, CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still loading a VST3. The rig can be stopped when it "
            "finishes."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    timelineOperationRunning.store(true, std::memory_order_release);
    // The closing state is captured before the instances are destroyed.
    juce::String closeError;
    if (!closeRigEditorAndWait(kRigEditorCloseTimeout, closeError)) {
        timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("rig", closeError, "rig.stop");
        return;
    }
    const auto submitted = context.runtimeLifecycle.submitWithQueueDeadline(
        [this, pending] {
            context.pipeline.rig().uninstall();
            timelineOperationRunning.store(false, std::memory_order_release);
            pending->respond(RigAckSpec{});
        },
        kRigCloseWatchdogTimeout, kRigCommandQueueTimeout,
        [this, pending] {
            timelineOperationRunning.store(false, std::memory_order_release);
            failExpiredRigCommand(pending, "rig.stop");
        });
    if (!submitted) {
        timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.", "rig.stop");
    }
}

void AudioCommandDispatcher::handle(const OpenRigEditorCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still loading a VST3. The rig editor can be opened when it "
            "finishes."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    const auto submitted = context.runtimeLifecycle.submitWithQueueDeadline(
        [this, command, pending] {
            auto rack = context.pipeline.rig().findRack(command.deviceId);
            if (rack == nullptr) {
                pending->fail("rig", "The rig effect is not loaded.", "rig.editor.open");
                return;
            }
            const auto rigId = context.pipeline.rig().rigId();
            if (rigId.isEmpty()) {
                pending->fail("rig", "The rig is not playing.", "rig.editor.open");
                return;
            }
            const auto sourcePluginPath = rack->status().path;
            if (rigEditor != nullptr) {
                rigEditor->close();
                if (!rigEditor->waitForClosingState(0)) {
                    pending->fail("rig", "The previous rig editor did not close in time.",
                                  "rig.editor.open");
                    return;
                }
                releaseRigEditor();
            }
            rigEditorRack = rack;
            rigEditor = std::make_shared<PluginEditorHost>(
                *rack,
                [this, rigId, deviceId = command.deviceId, rack] {
                    const auto stateKey = "rig-state:" + (rigId + ":" + deviceId).toStdString();
                    // State events are best-effort latest-value updates. Capacity
                    // drops and shutdown must never become unbounded control errors.
                    (void)context.runtimeLifecycle.submitState(
                        stateKey,
                        [this, rigId, deviceId, rack] {
                            auto& engine = context.pipeline.rig();
                            if (engine.rigId() != rigId || engine.findRack(deviceId) != rack)
                                return;
                            const auto revision = engine.revision();
                            juce::String error;
                            const auto state = capturePersistedState(*rack, error);
                            if (!state.has_value()) {
                                writeEvent(FaultSpec{{"rig", error, "rig.editor.capture", {}}});
                                return;
                            }
                            writeEvent(
                                RigPluginStateChangedSpec{rigId, deviceId, *state, revision});
                        },
                        kRigEditorWatchdogTimeout, RuntimeLifecycleExecutor::TaskThread::control);
                },
                [this, rigId, deviceId = command.deviceId, rack, sourcePluginPath](
                    const std::optional<PluginStateSpec>& state, const juce::String& error) {
                    auto& engine = context.pipeline.rig();
                    if (engine.rigId() != rigId || engine.findRack(deviceId) != rack) return;
                    // The closing state bypasses the best-effort state lane: once the editor is
                    // gone, a dropped or skipped update would never be captured again.
                    if (!state.has_value()) {
                        writeEvent(FaultSpec{{"rig", error, "rig.editor.capture", {}}});
                        return;
                    }
                    writeClosingRigPluginState(RigPluginStateChangedSpec{
                        rigId, deviceId, *state, engine.revision(), false, sourcePluginPath});
                },
                PluginEditorHost::ParameterCallback{}, PluginEditorHost::ClosedCallback{}, rack);
            rigEditorOpen.store(true, std::memory_order_release);
            juce::String editorError;
            bool opened = false;
            try {
                opened = rigEditor->open(editorError);
            } catch (const std::exception& exception) {
                editorError = "Rig VST3 editor opening raised an exception: " +
                              juce::String(exception.what());
            } catch (...) {
                editorError = "Rig VST3 editor opening failed with an unknown exception.";
            }
            if (!opened) {
                releaseRigEditor();
                pending->fail("rig", editorError, "rig.editor.open");
                return;
            }
            pending->respond(RigAckSpec{});
        },
        kRigEditorWatchdogTimeout, kRigCommandQueueTimeout,
        [pending] { failExpiredRigCommand(pending, "rig.editor.open"); });
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "rig.editor.open");
    }
}

void AudioCommandDispatcher::handle(const CloseRigEditorCommand& command,
                                    CommandResponder responder) {
    // Without an editor there is no closing state to capture and no reason to
    // wait for a running Graph load.
    if (!rigEditorOpen.load(std::memory_order_acquire)) {
        responder.respond(RigAckSpec{});
        return;
    }
    juce::String error;
    if (!closeRigEditorAndWait(
            std::chrono::milliseconds(static_cast<std::int64_t>(command.timeoutMs)), error)) {
        responder.fail("rig", error, "rig.editor.close");
        return;
    }
    responder.respond(RigAckSpec{});
}

bool AudioCommandDispatcher::openRig(const RigProjectionSpec& projection, juce::String& error) {
    auto& engine = context.pipeline.rig();
    const auto sampleRate = context.pipeline.getSampleRate();
    const auto blockSize = context.pipeline.getBlockSize();
    // A rebuilt chain destroys the instances the editor edits, so the editor
    // and its closing state are taken before the projection is installed.
    if (rigEditor != nullptr && !engine.keepsPreparedChain(projection, sampleRate, blockSize))
        closeRigEditor();
    return engine.install(projection, sampleRate, blockSize, error);
}

void AudioCommandDispatcher::closeRig() {
    closeRigEditor();
    context.pipeline.rig().uninstall();
}

void AudioCommandDispatcher::closeRigEditor() {
    if (rigEditor == nullptr) return;
    rigEditor->close();
    releaseRigEditor();
}

bool AudioCommandDispatcher::closeRigEditorAndWait(const std::chrono::milliseconds timeout,
                                                   juce::String& error) {
    if (!rigEditorOpen.load(std::memory_order_acquire)) return true;
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    // The editor shares the Message Thread with VST lifecycle tasks; closing
    // it waits until none of them runs.
    if (!context.runtimeLifecycle.waitForIdle(timeout)) {
        error = "The VST lifecycle executor did not become idle in time.";
        return false;
    }
    if (rigEditor != nullptr) {
        rigEditor->close();
        const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
            deadline - std::chrono::steady_clock::now());
        if (!rigEditor->waitForClosingState(
                static_cast<int>(std::clamp<std::int64_t>(remaining.count(), 0, INT_MAX)))) {
            error = "The closing rig plugin state could not be captured.";
            return false;
        }
        releaseRigEditor();
    }
    return true;
}

void AudioCommandDispatcher::releaseRigEditor() {
    rigEditorOpen.store(false, std::memory_order_release);
    rigEditor.reset();
    rigEditorRack.reset();
}

}  // namespace riffra
