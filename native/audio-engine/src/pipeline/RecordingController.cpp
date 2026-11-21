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
    : timeline(timelineIn) {
    stopWorker = std::thread([this] { stopWorkerLoop(); });
}

RecordingController::~RecordingController() {
    juce::String ignored;
    (void)stop(ignored);
    {
        const std::lock_guard guard(stopMutex);
        stopWorkerStopping = true;
    }
    stopWake.notify_all();
    if (stopWorker.joinable()) stopWorker.join();
}

RealtimeRequest RecordingController::start(const juce::File& directory, const int countInBeats,
                                           juce::String& error) {
    const juce::ScopedLock guard(lock);
    if (arrangeRecording != nullptr || processing) {
        error = "A recording is already active.";
        return RealtimeRequest::rejected;
    }
    {
        const std::lock_guard stopGuard(stopMutex);
        if (stopInProgress) {
            error = "A recording is already active.";
            return RealtimeRequest::rejected;
        }
    }
    auto candidate =
        ArrangeRecordingSession::create(directory, timeline.recordingConfiguration(), error);
    if (candidate == nullptr) return RealtimeRequest::rejected;

    if (!timeline.setRecordingSink(candidate.get())) {
        error = "The realtime recording sink could not be queued.";
        juce::String cleanupError;
        (void)candidate->cancel(cleanupError);
        if (cleanupError.isNotEmpty()) error << " " << cleanupError;
        return RealtimeRequest::queueFull;
    }
    const auto started = timeline.startRecording(countInBeats, error);
    if (started != RealtimeRequest::accepted) {
        if (!timeline.clearRecordingSink()) {
            arrangeRecording = std::move(candidate);
            error = "The recording sink was not retired within one second.";
            return RealtimeRequest::rejected;
        }
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

void RecordingController::setStopCompletion(StopCompletion completion) {
    const std::lock_guard guard(stopMutex);
    stopCompletion = std::move(completion);
}

RealtimeRequest RecordingController::beginStop(PendingRecordingStop& pending, juce::String& error) {
    {
        const juce::ScopedLock guard(lock);
        if (processing) {
            error = "The previous recording is still being processed.";
            return RealtimeRequest::rejected;
        }
    }
    {
        const std::lock_guard guard(stopMutex);
        if (stopWorkerStopping) {
            error = "The recording stop worker is stopping.";
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
        finishStop();
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    pending.commandSequence = *sequence;
    return RealtimeRequest::accepted;
}

RealtimeRequest RecordingController::requestStopAsync(bool& cancelCountIn, juce::String& error) {
    PendingRecordingStop pending;
    const auto requested = beginStop(pending, error);
    if (requested != RealtimeRequest::accepted) return requested;
    {
        const std::lock_guard guard(stopMutex);
        pendingStop = pending;
    }
    stopWake.notify_one();
    cancelCountIn = pending.cancelCountIn;
    return RealtimeRequest::accepted;
}

RealtimeRequest RecordingController::applyStop(const PendingRecordingStop& pending,
                                               juce::String& error) {
    timeline.waitForCommandApplied(pending.commandSequence);

    std::unique_ptr<ArrangeRecordingSession> detached;
    FinalizationDispatcher dispatcher;
    juce::String finalizationError;
    bool finalized = true;
    {
        const juce::ScopedLock guard(lock);
        if (pending.cancelCountIn) {
            if (!timeline.clearRecordingSink()) {
                error = "The recording sink was not retired within one second.";
                return RealtimeRequest::rejected;
            }
            if (arrangeRecording == nullptr) {
                cancelled.store(true, std::memory_order_release);
                return RealtimeRequest::accepted;
            }
            auto cancelling = std::move(arrangeRecording);
            const auto wasCancelled = cancelling->cancel(error);
            cancelled.store(wasCancelled, std::memory_order_release);
            return wasCancelled ? RealtimeRequest::accepted : RealtimeRequest::rejected;
        }

        finalized = timeline.finalizeRecording(finalizationError);
        if (!finalized) error = finalizationError;
        if (!timeline.clearRecordingSink()) {
            error = "The recording sink was not retired within one second.";
            return RealtimeRequest::rejected;
        }
        if (arrangeRecording == nullptr) {
            return finalized ? RealtimeRequest::accepted : RealtimeRequest::rejected;
        }

        detached = std::move(arrangeRecording);
        processing = true;
        finalizationStatus = recordingStatus(detached->summary());
        finalizationStatus->active = false;
        finalizationStatus->processing = true;
        dispatcher = finalizationDispatcher;
    }

    if (dispatcher != nullptr) dispatcher(std::move(detached), finalizationError);
    return finalized ? RealtimeRequest::accepted : RealtimeRequest::rejected;
}

void RecordingController::finishStop() {
    {
        const std::lock_guard guard(stopMutex);
        stopInProgress = false;
    }
    stopFinished.notify_all();
}

void RecordingController::stopWorkerLoop() {
    for (;;) {
        PendingRecordingStop pending;
        StopCompletion completion;
        {
            std::unique_lock guard(stopMutex);
            stopWake.wait(guard, [this] { return stopWorkerStopping || pendingStop.has_value(); });
            if (stopWorkerStopping && !pendingStop.has_value()) return;
            pending = *pendingStop;
            pendingStop.reset();
            completion = stopCompletion;
        }

        juce::String error;
        const auto completed = applyStop(pending, error);
        if (completion != nullptr) completion(completed == RealtimeRequest::accepted, error);
        finishStop();
    }
}

RealtimeRequest RecordingController::stop(juce::String& error) {
    PendingRecordingStop pending;
    const auto requested = beginStop(pending, error);
    if (requested != RealtimeRequest::accepted) return requested;
    const auto completed = applyStop(pending, error);
    finishStop();
    return completed;
}

bool RecordingController::waitForStopCompletion(const std::chrono::milliseconds timeout) {
    std::unique_lock guard(stopMutex);
    return stopFinished.wait_for(guard, timeout, [this] { return !stopInProgress; });
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
