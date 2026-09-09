#include "AudioCommandDispatcher.h"

#include <cstdlib>
#include <string>

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
    if (type == "shutdown") {
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
    if (type == "setEmergencyMute" || type == "setFeedbackProtection" ||
        type == "setEngineTransitionMute" || type == "setMasterGainDb")
        return dispatchSafety(command);
    if (type == "loadTimelineSnapshot" || type == "prepareTimelineSnapshot" ||
        type == "commitTimelineSnapshot" || type == "discardTimelineSnapshot")
        return dispatchTimeline(command);
    if (type == "setTrackDeviceBypassed" || type == "setTrackDeviceParameter" ||
        type == "getTrackDeviceStatus" || type == "getTrackDeviceParameters" ||
        type == "setTrackPluginState" || type == "setTrackDeviceProgram" ||
        type == "openTrackPluginEditor")
        return dispatchTrackDevice(command);
    if (type == "playTimeline" || type == "setTransportStarting" || type == "stopTimeline" ||
        type == "seekTimeline")
        return dispatchTransport(command);
    if (type == "enableMidiListening" || type == "disableMidiListening" ||
        type == "setLiveMidiTarget" || type == "sendTrackMidi" || type == "panicTrackMidi")
        return dispatchMidi(command);
    if (type == "startTakeComparison" || type == "switchTakeComparisonVariant" ||
        type == "stopTakeComparison" || type == "previewSample" || type == "stopPreview")
        return dispatchPreview(command);
    if (type == "recoverAudioDevice" || type == "setAudioDriver") return dispatchDevice(command);
    if (type == "startArrangeRecording" || type == "stopArrangeRecording")
        return dispatchRecording(command);
    if (type == "status") {
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    if (type == "meterStatus") {
        writeJson(AudioStatusBuilder::currentMeters(context.pipeline));
        return {};
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
