#include "RecordingController.h"

#include <utility>

#include "recording/ArrangeRecordingSession.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

RecordingStatusSpec recordingStatus(const ArrangeRecordingSummary& summary) {
    RecordingStatusSpec status;
    status.active = summary.active;
    status.directory = summary.directory;
    status.sampleRate = summary.sampleRate;
    status.samplesWritten = summary.samplesWritten;
    status.droppedMidiEvents = summary.droppedMidiEvents;
    status.droppedBlocks = summary.droppedBlocks;
    status.missingSamples = summary.missingSamples;
    status.rawAttemptedSamples = summary.rawAttemptedSamples;
    status.processedAttemptedSamples = summary.processedAttemptedSamples;
    status.rawDroppedBlocks = summary.rawDroppedBlocks;
    status.processedDroppedBlocks = summary.processedDroppedBlocks;
    status.rawMissingSamples = summary.rawMissingSamples;
    status.processedMissingSamples = summary.processedMissingSamples;
    status.rawDropoutStartSample = summary.rawDropoutStartSample;
    status.rawDropoutEndSample = summary.rawDropoutEndSample;
    status.processedDropoutStartSample = summary.processedDropoutStartSample;
    status.processedDropoutEndSample = summary.processedDropoutEndSample;
    status.recoveryStatus =
        summary.clean() ? RecoveryStatusSpec::clean : RecoveryStatusSpec::partial;
    return status;
}

}  // namespace

RecordingController::RecordingController(TimelineEngine& timelineIn) noexcept
    : timeline(timelineIn) {}

RecordingController::~RecordingController() {
    juce::String ignored;
    (void)stop(ignored);
}

RealtimeRequest RecordingController::start(const juce::File& directory, const int countInBeats,
                                           juce::String& error) {
    const juce::ScopedLock guard(lock);
    if (arrangeRecording != nullptr || pendingFinalization != nullptr || processing ||
        stopInProgress) {
        error = "A recording is already active.";
        return RealtimeRequest::rejected;
    }
    auto candidate =
        ArrangeRecordingSession::create(directory, timeline.recordingConfiguration(), error);
    if (candidate == nullptr) return RealtimeRequest::rejected;

    timeline.setRecordingSink(candidate.get());
    const auto started = timeline.startRecording(countInBeats, error);
    if (started != RealtimeRequest::accepted) {
        timeline.clearRecordingSink();
        juce::String cleanupError;
        (void)candidate->cancel(cleanupError);
        if (cleanupError.isNotEmpty()) error << " " << cleanupError;
        return started;
    }

    arrangeRecording = std::move(candidate);
    cancelled.store(false, std::memory_order_release);
    finalizationStatus.reset();
    return RealtimeRequest::accepted;
}

void RecordingController::setFinalizationDispatcher(FinalizationDispatcher dispatcher) {
    const juce::ScopedLock guard(lock);
    finalizationDispatcher = std::move(dispatcher);
}

RealtimeRequest RecordingController::requestStop(PendingRecordingStop& pending,
                                                 juce::String& error) {
    {
        const juce::ScopedLock guard(lock);
        if (processing) {
            error = "The previous recording is still being processed.";
            return RealtimeRequest::rejected;
        }
        if (pendingFinalization != nullptr) {
            error = "The previous recording is waiting for finalization.";
            return RealtimeRequest::rejected;
        }
        if (stopInProgress) {
            error = "The previous recording stop is still pending.";
            return RealtimeRequest::rejected;
        }
        stopInProgress = true;
    }

    pending.cancelCountIn = timeline.status().frame.recordingPhase == RecordingPhase::countingIn;
    const auto sequence = timeline.enqueueArrangeRecordingStop();
    if (!sequence.has_value()) {
        const juce::ScopedLock guard(lock);
        stopInProgress = false;
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    pending.commandSequence = *sequence;
    return RealtimeRequest::accepted;
}

RealtimeRequest RecordingController::completeStop(const PendingRecordingStop& pending,
                                                  juce::String& error) {
    timeline.waitForCommandApplied(pending.commandSequence);

    std::unique_ptr<ArrangeRecordingSession> detached;
    FinalizationDispatcher dispatcher;
    juce::String finalizationError;
    bool finalized = true;
    {
        const juce::ScopedLock guard(lock);
        if (pending.cancelCountIn) {
            timeline.clearRecordingSink();
            if (arrangeRecording == nullptr) {
                cancelled.store(true, std::memory_order_release);
                stopInProgress = false;
                return RealtimeRequest::accepted;
            }
            auto cancelling = std::move(arrangeRecording);
            const auto wasCancelled = cancelling->cancel(error);
            cancelled.store(wasCancelled, std::memory_order_release);
            stopInProgress = false;
            return wasCancelled ? RealtimeRequest::accepted : RealtimeRequest::rejected;
        }

        finalized = timeline.finalizeRecording(finalizationError);
        if (!finalized) error = finalizationError;
        timeline.clearRecordingSink();
        if (arrangeRecording == nullptr) {
            stopInProgress = false;
            return finalized ? RealtimeRequest::accepted : RealtimeRequest::rejected;
        }

        detached = std::move(arrangeRecording);
        processing = true;
        finalizationStatus = recordingStatus(detached->summary());
        finalizationStatus->active = false;
        finalizationStatus->processing = true;
        dispatcher = finalizationDispatcher;
        stopInProgress = false;
    }

    if (dispatcher != nullptr)
        dispatcher(std::move(detached), finalizationError);
    else {
        const juce::ScopedLock guard(lock);
        pendingFinalization = std::move(detached);
    }
    return finalized ? RealtimeRequest::accepted : RealtimeRequest::rejected;
}

RealtimeRequest RecordingController::stop(juce::String& error) {
    PendingRecordingStop pending;
    const auto requested = requestStop(pending, error);
    if (requested != RealtimeRequest::accepted) return requested;
    return completeStop(pending, error);
}

std::unique_ptr<ArrangeRecordingSession> RecordingController::takePendingFinalization() noexcept {
    const juce::ScopedLock guard(lock);
    return std::move(pendingFinalization);
}

void RecordingController::completeProcessing(const ArrangeRecordingSummary& summary,
                                             const juce::String& error) {
    const juce::ScopedLock guard(lock);
    finalizationStatus = recordingStatus(summary);
    finalizationStatus->active = false;
    if (error.isNotEmpty()) finalizationStatus->error = error;
    processing = false;
}

RecordingStatusSpec RecordingController::status() const {
    const juce::ScopedLock guard(lock);
    if (arrangeRecording != nullptr) return recordingStatus(arrangeRecording->summary());
    if (finalizationStatus.has_value()) return *finalizationStatus;
    RecordingStatusSpec idle;
    idle.cancelled = cancelled.load(std::memory_order_acquire);
    return idle;
}

}  // namespace riffra
