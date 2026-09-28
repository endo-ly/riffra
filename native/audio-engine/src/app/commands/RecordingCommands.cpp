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
    const auto started =
        context.timelineEngine.startRecording(command.countInBeats, recordingError);
    if (started != RealtimeRequest::accepted) {
        juce::String rollbackError;
        (void)context.pipeline.stopArrangeRecording(context.timelineEngine, rollbackError);
        (void)rejectUnlessAccepted(responder, started, "recording", recordingError,
                                   "recording.start");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopArrangeRecordingCommand&,
                                    CommandResponder responder) {
    juce::String recordingError;
    const auto cancelled = context.timelineEngine.cancelRecordingIfCountingIn(recordingError);
    if (cancelled == RealtimeRequest::queueFull) {
        responder.fail("realtimeQueueFull", recordingError, "recording.stop");
        return;
    }
    auto stopped = false;
    if (cancelled == RealtimeRequest::accepted) {
        if (rejectUnlessQueued(responder, context.timelineEngine.stop().has_value(),
                               "recording.stop"))
            return;
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
