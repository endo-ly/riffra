#include "../AudioCommandDispatcher.h"
#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "TimelineEngine.h"

namespace riffra {

CommandResult AudioCommandDispatcher::dispatchRecording(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "startArrangeRecording") {
        const auto directory = command.getProperty("directory", {}).toString();
        juce::String recordingError;
        const auto started = context.pipeline.startArrangeRecording(
            juce::File(directory), context.timelineEngine, recordingError);
        if (directory.isEmpty() || !started) {
            writeJson(makeError("recording", directory.isEmpty()
                                                 ? "Recording directory is required."
                                                 : recordingError));
            return {};
        }
        if (!context.timelineEngine.startRecording(
                static_cast<int>(command.getProperty("countInBeats", 0)), recordingError)) {
            juce::String rollbackError;
            (void)context.pipeline.stopArrangeRecording(context.timelineEngine, rollbackError);
            writeJson(makeError("recording", recordingError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "stopArrangeRecording") {
        const auto cancelledCountIn = context.timelineEngine.cancelRecordingIfCountingIn();

        juce::String recordingError;

        if (cancelledCountIn) {
            context.timelineEngine.stop();

            if (!context.pipeline.cancelArrangeRecording(context.timelineEngine, recordingError)) {
                writeJson(makeError("recording", recordingError));
                return {};
            }
        } else {
            if (!context.pipeline.stopArrangeRecording(context.timelineEngine, recordingError)) {
                writeJson(makeError("recording", recordingError));
                return {};
            }
        }

        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    return {};
}

}  // namespace riffra
