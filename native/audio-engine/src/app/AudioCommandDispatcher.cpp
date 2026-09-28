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

bool AudioCommandDispatcher::rejectUnlessAccepted(CommandResponder& responder,
                                                  const RealtimeRequest request,
                                                  const juce::String& kind,
                                                  const juce::String& error,
                                                  const juce::String& operation) {
    switch (request) {
        case RealtimeRequest::accepted:
            return false;
        case RealtimeRequest::rejected:
            responder.fail(kind, error, operation);
            return true;
        case RealtimeRequest::queueFull:
            responder.fail("realtimeQueueFull", error, operation);
            return true;
    }
    return true;
}

bool AudioCommandDispatcher::rejectUnlessQueued(CommandResponder& responder, const bool queued,
                                                const juce::String& operation) {
    if (queued) return false;
    responder.fail("realtimeQueueFull", "The realtime command queue is full.", operation);
    return true;
}

void AudioCommandDispatcher::handle(const StatusCommand&, CommandResponder responder) {
    responder.respond(currentStatus());
}

// Muting also silences instruments so held notes do not sound once unmuted.
void AudioCommandDispatcher::handle(const SetEmergencyMuteCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setUserEmergencyMute(command.muted);
    if (command.muted &&
        rejectUnlessQueued(responder, context.timelineEngine.panicAllInstrumentTracks().has_value(),
                           "audio.emergencyMute"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetFeedbackProtectionCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setFeedbackProtection(command.active);
    if (command.active &&
        rejectUnlessQueued(responder, context.timelineEngine.panicAllInstrumentTracks().has_value(),
                           "audio.feedbackProtection"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetEngineTransitionMuteCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setEngineTransitionMute(command.active);
    if (command.active &&
        rejectUnlessQueued(responder, context.timelineEngine.panicAllInstrumentTracks().has_value(),
                           "audio.engineTransitionMute"))
        return;
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const PreviewMasterGainDbCommand& command,
                                    CommandResponder responder) {
    context.pipeline.setMasterGainDb(static_cast<float>(command.gainDb));
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const TransportCommand& command, CommandResponder responder) {
    auto& timeline = context.timelineEngine;
    const auto accepted = [&]() -> std::optional<std::uint64_t> {
        switch (command.kind) {
            case TransportCommandKind::play:
                return timeline.play();
            case TransportCommandKind::setStarting:
                return timeline.startPreparing();
            case TransportCommandKind::stop:
                return timeline.stop();
            case TransportCommandKind::seek:
                return timeline.seekToTick(command.tick);
        }
        return std::nullopt;
    }();
    if (rejectUnlessQueued(responder, accepted.has_value(), "timeline.transport")) return;
    responder.respond(TransportAcceptedSpec{*accepted});
}

}  // namespace riffra
