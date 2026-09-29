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
    if (!reserveRecordingStop(responder)) return;

    juce::String recordingError;
    PendingRecordingStop pending;
    const auto requested = context.pipeline.requestArrangeRecordingStop(pending, recordingError);
    if (rejectUnlessAccepted(responder, requested, "recording", recordingError, "recording.stop")) {
        releaseRecordingStopReservation();
        return;
    }

    queueRecordingStop(pending);
    auto status = currentStatus();
    if (pending.cancelCountIn) status.recording.cancelled = true;
    responder.respond(status);
}

}  // namespace riffra
