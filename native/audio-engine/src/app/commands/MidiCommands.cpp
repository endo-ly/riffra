#include "../AudioCommandDispatcher.h"
#include "midi/MidiInputService.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::handle(const SetMidiListeningCommand& command,
                                    CommandResponder responder) {
    if (!command.listening) {
        juce::String error;
        if (!context.pipeline.stopPreview(&error)) {
            responder.fail("preview", error, "midi.listening");
            return;
        }
    }
    context.midiInputs.setListening(command.listening);
    context.midiInputs.monitor().setActive(command.listening);
    context.midiInputs.reopenAll();
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetLiveMidiTargetCommand& command,
                                    CommandResponder responder) {
    juce::String timelineError;
    const auto request = context.timelineEngine.setLiveMidiTarget(
        command.trackId.value_or(juce::String()), timelineError);
    if (rejectUnlessAccepted(responder, request, "liveMidiTarget", timelineError,
                             "midi.liveTarget"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SendTrackMidiCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still changing; targeted MIDI can be retried shortly."))
        return;
    juce::String timelineError;
    const auto request =
        context.timelineEngine.enqueueTargetedMidi(command.trackId, command.message, timelineError);
    if (rejectUnlessAccepted(responder, request, "targetedMidi", timelineError, "midi.send"))
        return;
    responder.respond(MidiAckSpec{});
}

void AudioCommandDispatcher::handle(const PanicTrackMidiCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still changing; targeted MIDI can be retried shortly."))
        return;
    juce::String timelineError;
    const auto request = context.timelineEngine.panicTargetedMidi(command.trackId, timelineError);
    if (rejectUnlessAccepted(responder, request, "targetedMidi", timelineError, "midi.panic"))
        return;
    responder.respond(MidiAckSpec{});
}

}  // namespace riffra
