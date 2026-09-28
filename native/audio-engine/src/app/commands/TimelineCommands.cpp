#include <chrono>
#include <memory>

#include "../AudioCommandDispatcher.h"
#include "plugins/PluginEditorHost.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

constexpr auto kTimelineVstLifecycleTimeout = std::chrono::seconds(45);

}  // namespace

void AudioCommandDispatcher::handle(const WaitForTimelineIdleCommand& command,
                                    CommandResponder responder) {
    if (!context.runtimeLifecycle.waitForIdle(
            std::chrono::milliseconds(static_cast<std::int64_t>(command.timeoutMs)))) {
        responder.fail("runtimeLifecycle",
                       "The VST lifecycle executor did not become idle in time.",
                       "runtime.timeline.waitForIdle");
        return;
    }
    responder.respond(AckSpec{});
}

void AudioCommandDispatcher::handle(const PrepareTimelineSnapshotCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "Another Arrangement Graph is still loading a VST3. The current "
                                "runtime remains available."))
        return;
    auto* device = context.deviceController.manager().getCurrentAudioDevice();
    const auto blockSize = device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
    const auto sampleRate = context.pipeline.getSampleRate();
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, snapshot = command.snapshot, pending, sampleRate, blockSize] {
            juce::String timelineError;
            bool loaded = false;
            try {
                loaded = context.timelineEngine.loadSnapshot(
                    snapshot, context.formatManager, sampleRate, blockSize, timelineError, false);
            } catch (const std::exception& exception) {
                timelineError = "Arrangement VST3 loading raised an exception: " +
                                juce::String(exception.what());
            } catch (...) {
                timelineError = "Arrangement VST3 loading failed with an unknown exception.";
            }
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (loaded)
                pending->respond(AckSpec{});
            else
                pending->fail("timeline", timelineError, "runtime.timeline.prepare");
        },
        kTimelineVstLifecycleTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "runtime.timeline.prepare");
    }
}

void AudioCommandDispatcher::handle(const CommitTimelineSnapshotCommand&,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3 and cannot be "
                                "committed yet."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, pending] {
            const auto shouldCloseEditor =
                context.timelineEngine.hasPreparedSnapshot() &&
                context.trackPluginEditor != nullptr &&
                !context.timelineEngine.preparedTrackReusesRuntimeDevices(
                    context.trackPluginEditorTrackId);
            if (shouldCloseEditor) {
                context.trackPluginEditor->close();
                context.trackPluginEditor.reset();
                context.trackPluginEditorTrackId.clear();
                context.trackPluginEditorDeviceId.clear();
            }
            juce::String timelineError;
            const auto committed = context.timelineEngine.commitPreparedSnapshot(timelineError);
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (!committed) {
                pending->fail("timeline", timelineError, "runtime.timeline.commit");
                return;
            }
            context.pipeline.setMasterGainDb(context.timelineEngine.activeMasterGainDb());
            pending->respond(AckSpec{});
        },
        kTimelineVstLifecycleTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "runtime.timeline.commit");
    }
}

void AudioCommandDispatcher::handle(const DiscardTimelineSnapshotCommand&,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3 and cannot be "
                                "discarded yet."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, pending] {
            context.timelineEngine.discardPreparedSnapshot();
            context.timelineOperationRunning.store(false, std::memory_order_release);
            pending->respond(AckSpec{});
        },
        std::chrono::seconds(5));
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "runtime.timeline.discard");
    }
}

}  // namespace riffra
