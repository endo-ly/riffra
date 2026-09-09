#include "../AudioCommandDispatcher.h"
#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "TimelineEngine.h"

namespace riffra {

CommandResult AudioCommandDispatcher::dispatchMidi(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "enableMidiListening") {
        context.midiInputs.setListening(true);
        context.midiInputs.reopenAll();
        context.midiInputs.monitor().setActive(true);
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "disableMidiListening") {
        context.midiInputs.setListening(false);
        context.midiInputs.monitor().setActive(false);
        context.pipeline.stopPreview();
        context.pipeline.allNotesOff();
        context.midiInputs.reopenAll();
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "setLiveMidiTarget") {
        const auto trackId = command.getProperty("trackId", {}).toString();
        juce::String timelineError;
        if (!context.timelineEngine.setLiveMidiTarget(trackId, timelineError)) {
            writeJson(makeError("liveMidiTarget", timelineError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "sendTrackMidi" || type == "panicTrackMidi") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still changing; targeted MIDI "
                                "can be retried shortly."));
            return {};
        }
        const auto trackId = command.getProperty("trackId", {}).toString();
        juce::String timelineError;
        bool accepted = false;
        if (type == "sendTrackMidi") {
            juce::MidiMessage message;
            juce::String midiError;
            if (!parseMidiBytes(command.getProperty("bytes", {}), message, midiError)) {
                writeJson(makeError("midi", midiError));
                return {};
            }
            accepted = context.timelineEngine.enqueueTargetedMidi(trackId, message, timelineError);
        } else {
            accepted = context.timelineEngine.panicTargetedMidi(trackId, timelineError);
        }
        if (!accepted) {
            writeJson(makeError("targetedMidi", timelineError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentMeters(context.pipeline));
        return {};
    }
    return {};
}

}  // namespace riffra
