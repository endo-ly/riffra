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

}  // namespace

void AudioCommandDispatcher::handle(const SetDeviceBypassedCommand& command,
                                    CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceBypassed(
                command.owner, command.deviceId, command.bypassed, deviceError);

            if (changed)
                pending->respond(DeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.bypass");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.bypass");
    }
}

void AudioCommandDispatcher::handle(const SetDeviceParameterCommand& command,
                                    CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submitControl(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceParameter(
                command.owner, command.deviceId, static_cast<int>(command.parameterIndex),
                command.value, deviceError, command.display);

            if (changed)
                pending->respond(DeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.parameter");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.parameter");
    }
}

void AudioCommandDispatcher::handle(const GetDeviceCommand& command, CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submitControl(
        [this, command, pending] {
            juce::String deviceError;
            std::optional<SidecarResponseSpec> result;
            switch (command.query) {
                case DeviceQuery::status:
                    if (auto status = context.timelineEngine.deviceStatus(
                            command.owner, command.deviceId, deviceError))
                        result = std::move(*status);
                    break;
                case DeviceQuery::parameters:
                    if (auto parameters = context.timelineEngine.deviceParameterStatus(
                            command.owner, command.deviceId, deviceError))
                        result = std::move(*parameters);
                    break;
                case DeviceQuery::programs:
                    if (auto programs = context.timelineEngine.deviceProgramStatus(
                            command.owner, command.deviceId, deviceError))
                        result = std::move(*programs);
                    break;
                case DeviceQuery::pluginState:
                    if (auto state = context.timelineEngine.devicePersistedState(
                            command.owner, command.deviceId, deviceError))
                        result = PluginStateResponseSpec{std::move(*state)};
                    break;
            }

            if (result.has_value())
                pending->respond(*result);
            else
                pending->fail("trackDevice", deviceError, "track.device.inspect");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.inspect");
    }
}

void AudioCommandDispatcher::handle(const SetPluginStateCommand& command,
                                    CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submitControl(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDevicePersistedState(
                command.owner, command.deviceId, command.state, deviceError);

            if (changed)
                pending->respond(DeviceAckSpec{});
            else
                pending->fail("trackDevice", deviceError, "track.device.setPluginState");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.setPluginState");
    }
}

void AudioCommandDispatcher::handle(const SetDeviceProgramCommand& command,
                                    CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submitControl(
        [this, command, pending] {
            juce::String deviceError;
            const auto changed = context.timelineEngine.setDeviceProgram(
                command.owner, command.deviceId, static_cast<int>(command.programIndex),
                deviceError);
            const auto state = changed ? context.timelineEngine.devicePersistedState(
                                             command.owner, command.deviceId, deviceError)
                                       : std::nullopt;

            if (state.has_value())
                pending->respond(DeviceProgramChangedSpec{*state});
            else
                pending->fail("trackDevice", deviceError, "track.device.setProgram");
        },
        kTrackDeviceTimeout);
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.device.setProgram");
    }
}

void AudioCommandDispatcher::handle(const OpenPluginEditorCommand& command,
                                    CommandResponder responder) {
    const auto pending = std::make_shared<CommandResponder>(std::move(responder));

    const auto submitted = context.runtimeLifecycle.submit(
        [this, command, pending] {
            auto* device = context.timelineEngine.findDevice(command.owner, command.deviceId);
            if (device == nullptr) {
                pending->fail("trackDevice", "Track Device was not found.",
                              "track.pluginEditor.open");
                return;
            }
            if (context.trackPluginEditor != nullptr) {
                context.trackPluginEditor->close();
                context.trackPluginEditor.reset();
            }
            context.pluginEditorOwner = command.owner;
            context.trackPluginEditorDeviceId = command.deviceId;
            context.trackPluginEditor = std::make_shared<PluginEditorHost>(
                *device,
                [this, command, device] {
                    const auto stateKey =
                        "track-state:" + (command.projectId + ":" + command.deviceId).toStdString();
                    // State events are best-effort latest-value updates. Capacity
                    // drops and shutdown must never become unbounded control errors.
                    (void)context.runtimeLifecycle.submitState(
                        stateKey,
                        [this, command, device] {
                            if (context.timelineEngine.findDevice(command.owner,
                                                                  command.deviceId) != device)
                                return;
                            juce::String error;
                            const auto state = context.timelineEngine.devicePersistedState(
                                command.owner, command.deviceId, error);
                            if (!state.has_value()) {
                                writeEvent(FaultSpec{
                                    {"trackDevice", error, "track.pluginEditor.capture", {}}});
                                return;
                            }
                            writeEvent(PluginStateChangedSpec{
                                command.projectId, command.owner, command.deviceId, *state,
                                context.timelineEngine.committedRevision()});
                        },
                        kTrackDeviceTimeout, RuntimeLifecycleExecutor::TaskThread::control);
                },
                [this, command, device](const int parameterIndex, const float value) {
                    const auto stateKey =
                        "track-parameter:" +
                        (command.projectId + ":" + command.deviceId).toStdString() + ":" +
                        std::to_string(parameterIndex);
                    // State events are best-effort latest-value updates. Capacity
                    // drops and shutdown must never become unbounded control errors.
                    (void)context.runtimeLifecycle.submitState(
                        stateKey,
                        [this, command, parameterIndex, value, device] {
                            if (context.timelineEngine.findDevice(command.owner,
                                                                  command.deviceId) != device)
                                return;
                            writeEvent(PluginParameterChangedSpec{
                                command.projectId, command.owner, command.deviceId,
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

            if (!opened) {
                context.trackPluginEditor.reset();
                context.pluginEditorOwner = {};
                context.trackPluginEditorDeviceId.clear();
                pending->fail("pluginEditor", editorError, "track.pluginEditor.open");
                return;
            }
            pending->respond(DeviceAckSpec{});
        },
        std::chrono::seconds(30));
    if (!submitted) {
        pending->fail("runtimeLifecycle", "The VST lifecycle executor is stopping.",
                      "track.pluginEditor.open");
    }
}

}  // namespace riffra
