#include "../AudioCommandDispatcher.h"
#include "midi/MidiInputService.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::handle(const SetMidiListeningCommand& command,
                                    CommandResponder responder) {
    context.midiInputs.setListening(command.listening);
    if (command.listening) {
        context.midiInputs.reopenAll();
        context.midiInputs.monitor().setActive(true);
    } else {
        context.midiInputs.monitor().setActive(false);
        context.pipeline.stopPreview();
        context.pipeline.allNotesOff();
        context.midiInputs.reopenAll();
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetLiveMidiTargetCommand& command,
                                    CommandResponder responder) {
    juce::String timelineError;
    if (!context.timelineEngine.setLiveMidiTarget(command.trackId.value_or(juce::String()),
                                                  timelineError)) {
        responder.fail("liveMidiTarget", timelineError, "midi.liveTarget");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SendTrackMidiCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still changing; targeted MIDI can be retried shortly."))
        return;
    juce::String timelineError;
    if (!context.timelineEngine.enqueueTargetedMidi(command.trackId, command.message,
                                                    timelineError)) {
        responder.fail("targetedMidi", timelineError, "midi.send");
        return;
    }
    responder.respond(MidiAckSpec{});
}

void AudioCommandDispatcher::handle(const PanicTrackMidiCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still changing; targeted MIDI can be retried shortly."))
        return;
    juce::String timelineError;
    if (!context.timelineEngine.panicTargetedMidi(command.trackId, timelineError)) {
        responder.fail("targetedMidi", timelineError, "midi.panic");
        return;
    }
    responder.respond(MidiAckSpec{});
}

}  // namespace riffra
