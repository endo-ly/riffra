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
    if (arrangeRecording != nullptr || pendingFinalization != nullptr || processing) {
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

RealtimeRequest RecordingController::stop(juce::String& error) {
    std::unique_ptr<ArrangeRecordingSession> detached;
    FinalizationDispatcher dispatcher;
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

        const auto cancelCountIn =
            timeline.status().frame.recordingPhase == RecordingPhase::countingIn;
        const auto stopped = timeline.stopArrangeRecording(error);
        if (stopped != RealtimeRequest::accepted) return stopped;

        if (cancelCountIn) {
            timeline.clearRecordingSink();
            if (arrangeRecording == nullptr) {
                cancelled.store(true, std::memory_order_release);
                return RealtimeRequest::accepted;
            }
            auto cancelling = std::move(arrangeRecording);
            const auto wasCancelled = cancelling->cancel(error);
            cancelled.store(wasCancelled, std::memory_order_release);
            return wasCancelled ? RealtimeRequest::accepted : RealtimeRequest::rejected;
        }

        if (!timeline.finalizeRecording(error)) return RealtimeRequest::rejected;
        timeline.clearRecordingSink();
        if (arrangeRecording == nullptr) return RealtimeRequest::accepted;

        detached = std::move(arrangeRecording);
        processing = true;
        finalizationStatus = recordingStatus(detached->summary());
        finalizationStatus->active = false;
        finalizationStatus->processing = true;
        dispatcher = finalizationDispatcher;
    }

    if (dispatcher != nullptr)
        dispatcher(std::move(detached));
    else {
        const juce::ScopedLock guard(lock);
        pendingFinalization = std::move(detached);
    }
    return RealtimeRequest::accepted;
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
