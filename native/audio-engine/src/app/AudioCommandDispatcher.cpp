#include "AudioCommandDispatcher.h"

#include <string>
#include <utility>
#include <variant>

#include "midi/MidiInputService.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void AudioCommandDispatcher::run(std::istream& input) {
    std::string line;
    while (std::getline(input, line)) {
        const auto envelope = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
        SidecarRequestSpec request;
        juce::String error;
        if (!decodeSidecarRequest(envelope, request, error)) {
            if (const auto requestId = readSidecarRequestId(envelope))
                CommandResponder(*requestId, writeControlEnvelope)
                    .fail("invalidCommand", error, "sidecar.decode");
            else
                writeEvent(FaultSpec{{"protocol", error, "sidecar.decode", {}}});
            continue;
        }
        dispatch(request, CommandResponder(request.requestId, writeControlEnvelope));
    }
}

void AudioCommandDispatcher::dispatch(const SidecarRequestSpec& request,
                                      CommandResponder responder) {
    std::visit([this, &responder](const auto& command) { handle(command, std::move(responder)); },
               request.command);
}

AudioStatusSpec AudioCommandDispatcher::currentStatus() const {
    return AudioStatusBuilder::currentStatus(context.deviceController.manager(), context.pipeline,
                                             context.midiInputs.monitor(), context.timelineEngine);
}

bool AudioCommandDispatcher::rejectWhileTimelineBusy(CommandResponder& responder,
                                                     const juce::String& message) {
    if (!context.timelineOperationRunning.load(std::memory_order_acquire)) return false;
    responder.fail("timelineBusy", message, "runtime.timeline");
    return true;
}

void AudioCommandDispatcher::handle(const StatusCommand&, CommandResponder responder) {
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetEmergencyMuteCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setUserEmergencyMute(command.muted);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetFeedbackProtectionCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setFeedbackProtection(command.active);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetEngineTransitionMuteCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setEngineTransitionMute(command.active);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const PreviewMasterGainDbCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setMasterGainDb(static_cast<float>(command.gainDb));
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const TransportCommand& command, CommandResponder responder) {
    switch (command.kind) {
        case TransportCommandKind::play:
            context.timelineEngine.play();
            break;
        case TransportCommandKind::setStarting:
            context.timelineEngine.startPreparing();
            break;
        case TransportCommandKind::stop:
            context.timelineEngine.stop();
            break;
        case TransportCommandKind::seek:
            context.timelineEngine.seekToTick(command.tick);
            break;
    }
    responder.respond(AudioStatusBuilder::currentTransport(context.timelineEngine));
}

}  // namespace riffra
