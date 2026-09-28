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
    const auto started = context.pipeline.startArrangeRecording(
        juce::File(command.directory), command.countInBeats, recordingError);
    if (rejectUnlessAccepted(responder, started, "recording", recordingError, "recording.start"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopArrangeRecordingCommand&,
                                    CommandResponder responder) {
    juce::String recordingError;
    const auto stopped = context.pipeline.stopArrangeRecording(recordingError);
    if (rejectUnlessAccepted(responder, stopped, "recording", recordingError, "recording.stop"))
        return;
    responder.respond(currentStatus());
}

}  // namespace riffra
