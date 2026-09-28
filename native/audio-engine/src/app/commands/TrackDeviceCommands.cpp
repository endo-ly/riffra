#include <chrono>
#include <memory>
#include <string>

#include "../AudioCommandDispatcher.h"
#include "plugins/PluginEditorHost.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

constexpr auto kTrackDeviceTimeout = std::chrono::seconds(10);
constexpr auto kTrackDeviceBusyMessage =
    "The Arrangement Graph is still loading a VST3. Track device changes can be retried shortly.";

void reportMirrorFailure(const juce::String& error) {
    writeEvent(FaultSpec{{"trackDevice", error, "track.pluginEditor.mirror", {}}});
}

}  // namespace

void AudioCommandDispatcher::handle(const SetTrackDeviceBypassedCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder, kTrackDeviceBusyMessage)) return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceBypassed(
                command.trackId, command.deviceId, command.bypassed, deviceError);
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (changed)
                pending->respond(TrackDeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.bypass");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.bypass");
    }
}

void AudioCommandDispatcher::handle(const SetTrackDeviceParameterCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder, kTrackDeviceBusyMessage)) return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceParameter(
                command.trackId, command.deviceId, static_cast<int>(command.parameterIndex),
                command.value, deviceError);
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (changed)
                pending->respond(TrackDeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.parameter");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.parameter");
    }
}

void AudioCommandDispatcher::handle(const GetTrackDeviceCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3. Track device "
                                "inspection can be retried shortly."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            std::optional<SidecarResponseSpec> result;
            switch (command.query) {
                case TrackDeviceQuery::status:
                    if (auto status = context.timelineEngine.deviceStatus(
                            command.trackId, command.deviceId, deviceError))
                        result = std::move(*status);
                    break;
                case TrackDeviceQuery::parameters:
                    if (auto parameters = context.timelineEngine.deviceParameterStatus(
                            command.trackId, command.deviceId, deviceError))
                        result = std::move(*parameters);
                    break;
                case TrackDeviceQuery::programs:
                    if (auto programs = context.timelineEngine.deviceProgramStatus(
                            command.trackId, command.deviceId, deviceError))
                        result = std::move(*programs);
                    break;
                case TrackDeviceQuery::pluginState:
                    if (auto state = context.timelineEngine.devicePersistedState(
                            command.trackId, command.deviceId, deviceError))
                        result = TrackPluginStateSpec{std::move(*state)};
                    break;
            }
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (result.has_value())
                pending->respond(*result);
            else
                pending->fail("trackDevice", deviceError, "track.device.inspect");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.inspect");
    }
}

void AudioCommandDispatcher::handle(const SetTrackPluginStateCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3. Track plugin "
                                "state changes can be retried shortly."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDevicePersistedState(
                command.trackId, command.deviceId, command.state, deviceError);
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (changed)
                pending->respond(TrackDeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.setPluginState");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.setPluginState");
    }
}

void AudioCommandDispatcher::handle(const SetTrackDeviceProgramCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3. Track plugin "
                                "programs can be changed shortly."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceProgram(
                command.trackId, command.deviceId, static_cast<int>(command.programIndex),
                deviceError);
            const auto state = changed ? context.timelineEngine.devicePersistedState(
                                             command.trackId, command.deviceId, deviceError)
                                       : std::nullopt;
            context.timelineOperationRunning.store(false, std::memory_order_release);
            if (state.has_value())
                pending->respond(TrackDeviceProgramChangedSpec{*state});
            else
                pending->fail("trackDevice", deviceError, "track.device.setProgram");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.setProgram");
    }
}

void AudioCommandDispatcher::handle(const OpenTrackPluginEditorCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3. The plugin editor "
                                "can be opened when it finishes."))
        return;
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));
    context.timelineOperationRunning.store(true, std::memory_order_release);
    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            auto* device = context.timelineEngine.findDevice(command.trackId, command.deviceId);
            if (device == nullptr) {
                context.timelineOperationRunning.store(false, std::memory_order_release);
                pending->fail("trackDevice", "Track Device was not found.",
                              "track.pluginEditor.open");
                return;
            }
            if (context.trackPluginEditor != nullptr) {
                context.trackPluginEditor->close();
                context.trackPluginEditor.reset();
            }
            context.trackPluginEditorTrackId = command.trackId;
            context.trackPluginEditorDeviceId = command.deviceId;
            context.trackPluginEditor = std::make_shared<PluginEditorHost>(
                *device,
                [this, command](const PluginStateSpec& state) {
                    const auto stateKey =
                        "track-state:" + (command.trackId + ":" + command.deviceId).toStdString();
                    // State events are best-effort latest-value updates. Capacity
                    // drops and shutdown must never become unbounded control errors.
                    (void)context.runtimeLifecycle.submitState(
                        stateKey,
                        [this, command, state] {
                            juce::String mirrorError;
                            if (!context.timelineEngine.mirrorEditorDeviceState(
                                    command.trackId, command.deviceId, state, mirrorError))
                                reportMirrorFailure(mirrorError);
                            writeEvent(TrackPluginStateChangedSpec{
                                command.projectId, command.trackId, command.deviceId, state});
                        },
                        kTrackDeviceTimeout);
                },
                [this, command](const int parameterIndex, const float value) {
                    const auto stateKey = "track-parameter:" +
                                          (command.trackId + ":" + command.deviceId).toStdString() +
                                          ":" + std::to_string(parameterIndex);
                    // State events are best-effort latest-value updates. Capacity
                    // drops and shutdown must never become unbounded control errors.
                    (void)context.runtimeLifecycle.submitState(
                        stateKey,
                        [this, command, parameterIndex, value] {
                            juce::String mirrorError;
                            if (!context.timelineEngine.mirrorEditorDeviceParameter(
                                    command.trackId, command.deviceId, parameterIndex, value,
                                    mirrorError)) {
                                reportMirrorFailure(mirrorError);
                                return;
                            }
                            writeEvent(TrackPluginParameterChangedSpec{
                                command.projectId, command.trackId, command.deviceId,
                                static_cast<std::uint32_t>(parameterIndex), value});
                        },
                        kTrackDeviceTimeout);
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
                pending->fail("pluginEditor", editorError, "track.pluginEditor.open");
                return;
            }
            pending->respond(TrackDeviceAckSpec{});
        },
        std::chrono::seconds(30));
    if (!submitted) {
        context.timelineOperationRunning.store(false, std::memory_order_release);
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.pluginEditor.open");
    }
}

}  // namespace riffra
