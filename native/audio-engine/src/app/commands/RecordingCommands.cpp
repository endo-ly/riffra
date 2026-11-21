#include "app/AudioCommandDispatcher.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::handle(const StartArrangeRecordingCommand& command,
                                    CommandResponder responder) {
    if (command.directory.isEmpty()) {
        responder.fail("recording", "Recording directory is required.", "recording.start");
        return;
    }
    juce::String recordingError;
    const auto started = context.pipeline.recording().start(juce::File(command.directory),
                                                            command.countInBeats, recordingError);
    if (rejectUnlessAccepted(responder, started, "recording", recordingError, "recording.start"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopArrangeRecordingCommand&,
                                    CommandResponder responder) {
    juce::String recordingError;
    bool cancelCountIn = false;
    const auto requested =
        context.pipeline.recording().requestStopAsync(cancelCountIn, recordingError);
    if (rejectUnlessAccepted(responder, requested, "recording", recordingError, "recording.stop"))
        return;

    auto status = currentStatus();
    if (cancelCountIn) status.recording.cancelled = true;
    responder.respond(status);
}

}  // namespace riffra
