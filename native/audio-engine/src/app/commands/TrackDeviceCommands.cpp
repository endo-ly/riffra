#include "../AudioCommandDispatcher.h"
#include "protocol/AudioProtocol.h"
#include "plugins/PluginEditorHost.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

CommandResult AudioCommandDispatcher::dispatchTrackDevice(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "setTrackDeviceBypassed") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Track "
                                "device changes can be retried shortly."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto trackId = command.getProperty("trackId", {}).toString();
        const auto deviceId = command.getProperty("deviceId", {}).toString();
        const auto bypassed = static_cast<bool>(command.getProperty("bypassed", false));
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, trackId, deviceId, bypassed] {
                juce::String deviceError;
                const auto changed = context.timelineEngine.setDeviceBypassed(
                    trackId, deviceId, bypassed, deviceError);
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (!changed) {
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                writeJson(context.timelineEngine.status(), requestId);
            },
            std::chrono::seconds(10));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "setTrackDeviceParameter") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Track "
                                "device changes can be retried shortly."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto trackId = command.getProperty("trackId", {}).toString();
        const auto deviceId = command.getProperty("deviceId", {}).toString();
        const auto parameterIndex = static_cast<int>(command.getProperty("parameterIndex", -1));
        const auto value = static_cast<float>(command.getProperty("value", 0.0));
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, trackId, deviceId, parameterIndex, value] {
                juce::String deviceError;
                const auto changed = context.timelineEngine.setDeviceParameter(
                    trackId, deviceId, parameterIndex, value, deviceError);
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (!changed) {
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                writeJson(context.timelineEngine.status(), requestId);
            },
            std::chrono::seconds(10));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "getTrackDeviceStatus" || type == "getTrackDeviceParameters" ||
        type == "getTrackDevicePrograms" || type == "getTrackPluginState") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Track "
                                "device inspection can be retried shortly."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto queryType = type;
        const auto trackId = command.getProperty("trackId", {}).toString();
        const auto deviceId = command.getProperty("deviceId", {}).toString();
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, queryType, trackId, deviceId] {
                juce::String deviceError;
                juce::var result;
                if (queryType == "getTrackDeviceStatus") {
                    result = context.timelineEngine.deviceStatus(trackId, deviceId, deviceError);
                } else if (queryType == "getTrackDeviceParameters") {
                    result = context.timelineEngine.deviceParameterStatus(trackId, deviceId,
                                                                          deviceError);
                } else if (queryType == "getTrackDevicePrograms") {
                    result =
                        context.timelineEngine.deviceProgramStatus(trackId, deviceId, deviceError);
                } else {
                    const auto state =
                        context.timelineEngine.devicePersistedState(trackId, deviceId, deviceError);
                    if (!state.isVoid()) {
                        auto* response = new juce::DynamicObject();
                        response->setProperty("type", "trackPluginState");
                        response->setProperty("state", state);
                        result = juce::var(response);
                    }
                }
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (result.isVoid()) {
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                writeJson(result, requestId);
            },
            std::chrono::seconds(10));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "setTrackPluginState") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Track "
                                "plugin state changes can be retried shortly."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto trackId = command.getProperty("trackId", {}).toString();
        const auto deviceId = command.getProperty("deviceId", {}).toString();
        const auto state = command.getProperty("state", {});
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, trackId, deviceId, state] {
                juce::String deviceError;
                const auto changed = context.timelineEngine.setDevicePersistedState(
                    trackId, deviceId, state, deviceError);
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (!changed) {
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                writeJson(context.timelineEngine.status(), requestId);
            },
            std::chrono::seconds(10));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "setTrackDeviceProgram") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Track "
                                "plugin programs can be changed shortly."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto trackId = command.getProperty("trackId", {}).toString();
        const auto deviceId = command.getProperty("deviceId", {}).toString();
        const auto programIndex =
            static_cast<int>(command.getProperty("programIndex", static_cast<juce::int64>(-1)));
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, trackId, deviceId, programIndex] {
                juce::String deviceError;
                const auto changed = context.timelineEngine.setDeviceProgram(
                    trackId, deviceId, programIndex, deviceError);
                if (!changed) {
                    context.timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                const auto program =
                    context.timelineEngine.deviceProgramStatus(trackId, deviceId, deviceError);
                const auto state =
                    context.timelineEngine.devicePersistedState(trackId, deviceId, deviceError);
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (program.isVoid() || state.isVoid()) {
                    writeJson(makeError("trackDevice", deviceError), requestId);
                    return;
                }
                auto* response = new juce::DynamicObject();
                response->setProperty("type", "trackDeviceProgramChanged");
                response->setProperty("program", program);
                response->setProperty("state", state);
                writeJson(juce::var(response), requestId);
            },
            std::chrono::seconds(10));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }

    if (type == "openTrackPluginEditor") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. The plugin "
                                "editor can be opened when it finishes."));
            return {};
        }
        const auto requestId = currentRequestId();
        const auto editorProjectId = command.getProperty("projectId", {}).toString();
        const auto editorTrackId = command.getProperty("trackId", {}).toString();
        const auto editorDeviceId = command.getProperty("deviceId", {}).toString();
        context.timelineOperationRunning.store(true, std::memory_order_release);
        const auto submitted = context.runtimeLifecycle.submit(
            [&, requestId, editorProjectId, editorTrackId, editorDeviceId] {
                auto* device = context.timelineEngine.findDevice(editorTrackId, editorDeviceId);
                if (device == nullptr) {
                    context.timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(makeError("trackDevice", "Track Device was not found."), requestId);
                    return;
                }
                if (context.trackPluginEditor != nullptr) {
                    context.trackPluginEditor->close();
                    context.trackPluginEditor.reset();
                }
                context.trackPluginEditorTrackId = editorTrackId;
                context.trackPluginEditorDeviceId = editorDeviceId;
                context.trackPluginEditor = std::make_shared<PluginEditorHost>(
                    *device,
                    [&, editorProjectId, editorTrackId, editorDeviceId](const juce::var& state) {
                        const auto stateCopy = state;
                        const auto stateKey =
                            "track-state:" + (editorTrackId + ":" + editorDeviceId).toStdString();
                        // State events are best-effort latest-value updates. Capacity
                        // drops and shutdown must never become unbounded control errors.
                        (void)context.runtimeLifecycle.submitState(
                            stateKey,
                            [&, editorProjectId, editorTrackId, editorDeviceId, stateCopy,
                             stateKey] {
                                juce::String mirrorError;
                                if (!context.timelineEngine.mirrorEditorDeviceState(
                                        editorTrackId, editorDeviceId, stateCopy, mirrorError)) {
                                    writeJson(makeError("trackDevice", mirrorError));
                                }
                                auto* changed = new juce::DynamicObject();
                                changed->setProperty("type", "trackPluginStateChanged");
                                changed->setProperty("projectId", editorProjectId);
                                changed->setProperty("trackId", editorTrackId);
                                changed->setProperty("deviceId", editorDeviceId);
                                changed->setProperty(
                                    "parameterValues",
                                    stateCopy.getProperty("parameterValues",
                                                          juce::Array<juce::var>{}));
                                changed->setProperty("stateData",
                                                     stateCopy.getProperty("stateData", {}));
                                changed->setProperty("bypassed",
                                                     stateCopy.getProperty("bypassed", false));
                                writeJson(juce::var(changed), {}, OutputKind::state, stateKey);
                            },
                            std::chrono::seconds(10));
                    },
                    [&, editorProjectId, editorTrackId, editorDeviceId](const int parameterIndex,
                                                                        const float value) {
                        const auto stateKey = "track-parameter:" +
                                              (editorTrackId + ":" + editorDeviceId).toStdString() +
                                              ":" + std::to_string(parameterIndex);
                        // State events are best-effort latest-value updates. Capacity
                        // drops and shutdown must never become unbounded control errors.
                        (void)context.runtimeLifecycle.submitState(
                            stateKey,
                            [&, editorProjectId, editorTrackId, editorDeviceId, parameterIndex,
                             value, stateKey] {
                                juce::String mirrorError;
                                if (!context.timelineEngine.mirrorEditorDeviceParameter(
                                        editorTrackId, editorDeviceId, parameterIndex, value,
                                        mirrorError)) {
                                    writeJson(makeError("trackDevice", mirrorError));
                                    return;
                                }
                                auto* changed = new juce::DynamicObject();
                                changed->setProperty("type", "trackPluginParameterChanged");
                                changed->setProperty("projectId", editorProjectId);
                                changed->setProperty("trackId", editorTrackId);
                                changed->setProperty("deviceId", editorDeviceId);
                                changed->setProperty("parameterIndex", parameterIndex);
                                changed->setProperty("value", value);
                                writeJson(juce::var(changed), {}, OutputKind::state, stateKey);
                            },
                            std::chrono::seconds(10));
                    });
                juce::String editorError;
                bool opened = false;
                try {
                    opened = context.trackPluginEditor->open(editorError);
                } catch (const std::exception& exception) {
                    editorError = "Track VST3 editor opening raised an exception: " +
                                  juce::String(exception.what());
                } catch (...) {
                    editorError = "Track VST3 editor opening failed with an unknown exception.";
                }
                context.timelineOperationRunning.store(false, std::memory_order_release);
                if (!opened) {
                    context.trackPluginEditor.reset();
                    context.trackPluginEditorTrackId.clear();
                    context.trackPluginEditorDeviceId.clear();
                    writeJson(makeError("pluginEditor", editorError), requestId);
                    return;
                }
                writeJson(context.timelineEngine.status(), requestId);
            },
            std::chrono::seconds(30));
        if (!submitted) {
            context.timelineOperationRunning.store(false, std::memory_order_release);
            writeJson(makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
        }
        return {};
    }
    return {};
}

}  // namespace riffra
