#include <chrono>

#include "../AudioCommandDispatcher.h"
#include "AudioProtocol.h"
#include "PluginEditorHost.h"
#include "TimelineEngine.h"

namespace riffra {
namespace {

constexpr auto kTimelineVstLifecycleTimeout = std::chrono::seconds(45);

}  // namespace

CommandResult AudioCommandDispatcher::dispatchTimeline(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "loadTimelineSnapshot" || type == "prepareTimelineSnapshot") {
        if (static_cast<int>(command.getProperty("protocolVersion", 0)) != 1) {
            writeJson(makeError("timelineProtocol", "Unsupported timeline protocol version."));
            return {};
        }
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "Another Arrangement Graph is still loading a VST3. The "
                                "current runtime remains available."));
            return {};
        }
        const auto commitImmediately = type == "loadTimelineSnapshot";
        auto* device = context.deviceController.manager().getCurrentAudioDevice();
        const auto blockSize = device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
        const auto snapshot = command.getProperty("snapshot", {});
        const auto sampleRate = context.pipeline.getSampleRate();
        const auto requestId = currentRequestId();
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, snapshot, requestId, sampleRate, blockSize, commitImmediately] {
                if (commitImmediately && context.trackPluginEditor != nullptr) {
                    context.trackPluginEditor->close();
                    context.trackPluginEditor.reset();
                    context.trackPluginEditorTrackId.clear();
                    context.trackPluginEditorDeviceId.clear();
                }
                juce::String timelineError;
                bool loaded = false;
                try {
                    loaded = context.timelineEngine.loadSnapshot(snapshot, context.formatManager,
                                                                 sampleRate, blockSize,
                                                                 timelineError, commitImmediately);
                } catch (const std::exception& exception) {
                    timelineError = "Arrangement VST3 loading raised an exception: " +
                                    juce::String(exception.what());
                } catch (...) {
                    timelineError = "Arrangement VST3 loading failed with an unknown exception.";
                }
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (!loaded) {
                    writeJson(makeError("timeline", timelineError), requestId);
                } else {
                    auto* ack = new juce::DynamicObject();
                    ack->setProperty("type", "timelineAck");
                    ack->setProperty("revision", snapshot.getProperty("revision", 0));
                    ack->setProperty(
                        "appliedAtAudioClockSample",
                        context.timelineEngine.status().getProperty("audioClockSample", 0));
                    ack->setProperty(
                        "unavailableClipIds",
                        snapshot.getProperty("unavailableClipIds", juce::Array<juce::var>{}));
                    writeJson(juce::var(ack), requestId);
                }
            },
            kTimelineVstLifecycleTimeout);
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "commitTimelineSnapshot") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3 and cannot "
                                "be committed yet."));
            return {};
        }
        const auto requestId = currentRequestId();
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId] {
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
                    writeJson(makeError("timeline", timelineError), requestId);
                    return;
                }
                writeJson(context.timelineEngine.status(), requestId);
            },
            kTimelineVstLifecycleTimeout);
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "discardTimelineSnapshot") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3 and cannot "
                                "be discarded yet."));
            return {};
        }
        const auto requestId = currentRequestId();
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId] {
                context.timelineEngine.discardPreparedSnapshot();
                context.timelineOperationRunning.store(false, std::memory_order_release);
                writeJson(context.timelineEngine.status(), requestId);
            },
            std::chrono::seconds(5));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }
    return {};
}

}  // namespace riffra
