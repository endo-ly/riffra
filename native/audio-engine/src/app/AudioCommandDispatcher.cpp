#include "AudioCommandDispatcher.h"

#include <cstdlib>
#include <string>

#include "CommandRouting.h"
#include "midi/MidiInputService.h"
#include "plugins/PluginEditorHost.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::run(std::istream& input) {
    std::string line;
    while (std::getline(input, line)) {
        clearCurrentRequestId();
        const auto command = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
        if (!command.isObject()) {
            writeJson(makeError("protocol", "Expected one JSON object per line."));
            continue;
        }
        setCurrentRequestId(command.getProperty("requestId", {}).toString());
        if (dispatch(command).shutdown) break;
    }
}

CommandResult AudioCommandDispatcher::dispatch(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    switch (commandFamilyFor(type.toStdString())) {
        case CommandFamily::shutdown: {
            context.pipeline.setEngineTransitionMute(true);
            const auto submitted = context.runtimeLifecycle.submit(
                [&] {
                    if (context.trackPluginEditor != nullptr) {
                        context.trackPluginEditor->close();
                        context.trackPluginEditor.reset();
                        context.trackPluginEditorTrackId.clear();
                        context.trackPluginEditorDeviceId.clear();
                    }
                    context.timelineOperationRunning.store(false, std::memory_order_release);
                },
                std::chrono::seconds(10));
            if (submitted && !context.runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
                std::_Exit(125);
            return {true};
        }
        case CommandFamily::safety:
            return dispatchSafety(command);
        case CommandFamily::timeline:
            return dispatchTimeline(command);
        case CommandFamily::trackDevice:
            return dispatchTrackDevice(command);
        case CommandFamily::transport:
            return dispatchTransport(command);
        case CommandFamily::midi:
            return dispatchMidi(command);
        case CommandFamily::preview:
            return dispatchPreview(command);
        case CommandFamily::device:
            return dispatchDevice(command);
        case CommandFamily::recording:
            return dispatchRecording(command);
        case CommandFamily::status:
            if (type == "status") {
                writeJson(AudioStatusBuilder::currentStatus(
                    context.deviceController.manager(), context.pipeline,
                    &context.midiInputs.monitor(), {}, &context.timelineEngine));
                return {};
            }
            writeJson(AudioStatusBuilder::currentMeters(context.pipeline));
            return {};
        case CommandFamily::unsupported:
            break;
    }
    writeJson(makeError("protocol", "Unsupported command: " + type));
    return {};
}

CommandResult AudioCommandDispatcher::dispatchSafety(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "setEmergencyMute") {
        const auto mutedValue = command.getProperty("muted", {});
        if (!mutedValue.isBool()) {
            writeJson(makeError("invalidCommand",
                                "setEmergencyMute requires a boolean muted field.",
                                "safety.userEmergencyMute"));
            return {};
        }
        const auto muted = static_cast<bool>(mutedValue);
        context.pipeline.setUserEmergencyMute(muted);
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "setFeedbackProtection") {
        const auto active = static_cast<bool>(command.getProperty("active", false));
        context.pipeline.setFeedbackProtection(active);
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "setEngineTransitionMute") {
        const auto active = static_cast<bool>(command.getProperty("active", true));
        context.pipeline.setEngineTransitionMute(active);
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "setMasterGainDb") {
        context.pipeline.setMasterGainDb(
            static_cast<float>(command.getProperty("gainDb", context.pipeline.getMasterGainDb())));
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    return {};
}

CommandResult AudioCommandDispatcher::dispatchTransport(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "playTimeline") {
        context.timelineEngine.play();
        writeJson(context.timelineEngine.status());
        return {};
    }

    if (type == "setTransportStarting") {
        context.timelineEngine.startPreparing();
        writeJson(context.timelineEngine.status());
        return {};
    }

    if (type == "stopTimeline") {
        context.timelineEngine.stop();
        if (static_cast<bool>(command.getProperty("reportStatus", true)))
            writeJson(context.timelineEngine.status());
        return {};
    }

    if (type == "seekTimeline") {
        const auto tick =
            static_cast<std::uint64_t>(static_cast<juce::int64>(command.getProperty("tick", 0)));
        context.timelineEngine.seekToTick(tick);
        writeJson(context.timelineEngine.status());
        return {};
    }
    return {};
}

}  // namespace riffra
