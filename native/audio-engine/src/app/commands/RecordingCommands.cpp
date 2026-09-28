#include "../AudioCommandDispatcher.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::handle(const StartArrangeRecordingCommand& command,
                                    CommandResponder responder) {
    if (command.directory.isEmpty()) {
        responder.fail("recording", "Recording directory is required.", "recording.start");
        return;
    }
    juce::String recordingError;
    if (!context.pipeline.startArrangeRecording(juce::File(command.directory),
                                                context.timelineEngine, recordingError)) {
        responder.fail("recording", recordingError, "recording.start");
        return;
    }
    if (!context.timelineEngine.startRecording(command.countInBeats, recordingError)) {
        juce::String rollbackError;
        (void)context.pipeline.stopArrangeRecording(context.timelineEngine, rollbackError);
        responder.fail("recording", recordingError, "recording.start");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopArrangeRecordingCommand&,
                                    CommandResponder responder) {
    juce::String recordingError;
    auto stopped = false;
    if (context.timelineEngine.cancelRecordingIfCountingIn()) {
        context.timelineEngine.stop();
        stopped = context.pipeline.cancelArrangeRecording(context.timelineEngine, recordingError);
    } else {
        stopped = context.pipeline.stopArrangeRecording(context.timelineEngine, recordingError);
    }
    if (!stopped) {
        responder.fail("recording", recordingError, "recording.stop");
        return;
    }
    responder.respond(currentStatus());
}

}  // namespace riffra
