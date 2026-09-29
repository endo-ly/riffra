#include "AudioCommandDispatcher.h"

#include <chrono>
#include <string>
#include <utility>
#include <variant>

#include "midi/MidiInputService.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

constexpr auto kSafetyMutePanicApplyTimeout = std::chrono::milliseconds(500);

}  // namespace

AudioCommandDispatcher::AudioCommandDispatcher(Context context) : context(context) {
    recordingStopWorker = std::thread([this] { recordingStopWorkerLoop(); });
}

AudioCommandDispatcher::~AudioCommandDispatcher() {
    {
        const std::lock_guard lock(recordingStopMutex);
        recordingStopWorkerStopping = true;
    }
    recordingStopWake.notify_all();
    if (recordingStopWorker.joinable()) recordingStopWorker.join();
}

bool AudioCommandDispatcher::waitForBackgroundWork(const std::chrono::milliseconds timeout) {
    std::unique_lock lock(recordingStopMutex);
    return recordingStopFinished.wait_for(lock, timeout,
                                          [this] { return !recordingStopScheduled; });
}

bool AudioCommandDispatcher::reserveRecordingStop(CommandResponder& responder) {
    const std::lock_guard lock(recordingStopMutex);
    if (!recordingStopScheduled && !recordingStopWorkerStopping) {
        recordingStopScheduled = true;
        return true;
    }
    responder.fail("recordingBusy", "A recording stop is already pending.", "recording.stop");
    return false;
}

void AudioCommandDispatcher::releaseRecordingStopReservation() {
    {
        const std::lock_guard lock(recordingStopMutex);
        recordingStopScheduled = false;
    }
    recordingStopFinished.notify_all();
}

void AudioCommandDispatcher::queueRecordingStop(const PendingRecordingStop& pending) {
    {
        const std::lock_guard lock(recordingStopMutex);
        pendingRecordingStop = pending;
    }
    recordingStopWake.notify_one();
}

void AudioCommandDispatcher::recordingStopWorkerLoop() {
    for (;;) {
        PendingRecordingStop pending;
        {
            std::unique_lock lock(recordingStopMutex);
            recordingStopWake.wait(lock, [this] {
                return recordingStopWorkerStopping || pendingRecordingStop.has_value();
            });
            if (recordingStopWorkerStopping && !pendingRecordingStop.has_value()) return;
            pending = *pendingRecordingStop;
            pendingRecordingStop.reset();
        }

        juce::String error;
        const auto completed = context.pipeline.completeArrangeRecordingStop(pending, error);
        if (completed != RealtimeRequest::accepted)
            writeEvent(FaultSpec{{"recording", error, "recording.stop", {}}});

        {
            const std::lock_guard lock(recordingStopMutex);
            recordingStopScheduled = false;
        }
        recordingStopFinished.notify_all();
    }
}

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
    if (command.muted) {
        context.pipeline.setUserEmergencyMute(true);
        (void)context.timelineEngine.panicAllInstrumentTracks();
    } else if (context.pipeline.hasMuteReason(MuteReason::UserEmergency)) {
        const auto panic = context.timelineEngine.panicAllInstrumentTracks();
        if (rejectUnlessQueued(responder, panic.has_value(), "audio.emergencyMute")) return;
        if (!context.timelineEngine.waitForCommandApplied(*panic, kSafetyMutePanicApplyTimeout)) {
            responder.fail("timeout", "The panic command was not applied; the mute remains on.",
                           "audio.emergencyMute");
            return;
        }
    }
    if (!command.muted) context.pipeline.setUserEmergencyMute(false);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetFeedbackProtectionCommand& command,
                                    CommandResponder responder) {
    if (command.active) {
        context.pipeline.setFeedbackProtection(true);
        (void)context.timelineEngine.panicAllInstrumentTracks();
    } else if (context.pipeline.hasMuteReason(MuteReason::FeedbackProtection)) {
        const auto panic = context.timelineEngine.panicAllInstrumentTracks();
        if (rejectUnlessQueued(responder, panic.has_value(), "audio.feedbackProtection")) return;
        if (!context.timelineEngine.waitForCommandApplied(*panic, kSafetyMutePanicApplyTimeout)) {
            responder.fail("timeout", "The panic command was not applied; the mute remains on.",
                           "audio.feedbackProtection");
            return;
        }
    }
    if (!command.active) context.pipeline.setFeedbackProtection(false);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SetEngineTransitionMuteCommand& command,
                                    CommandResponder responder) {
    if (command.active) {
        context.pipeline.setEngineTransitionMute(true);
        (void)context.timelineEngine.panicAllInstrumentTracks();
    } else if (context.pipeline.hasMuteReason(MuteReason::EngineTransition)) {
        const auto panic = context.timelineEngine.panicAllInstrumentTracks();
        if (rejectUnlessQueued(responder, panic.has_value(), "audio.engineTransitionMute")) return;
        if (!context.timelineEngine.waitForCommandApplied(*panic, kSafetyMutePanicApplyTimeout)) {
            responder.fail("timeout", "The panic command was not applied; the mute remains on.",
                           "audio.engineTransitionMute");
            return;
        }
    }
    if (!command.active) context.pipeline.setEngineTransitionMute(false);
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
