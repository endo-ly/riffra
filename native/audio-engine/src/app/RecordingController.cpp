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

bool RecordingController::start(const juce::File& directory, juce::String& error) {
    const juce::ScopedLock guard(lock);
    if (arrangeRecording != nullptr || pendingFinalization != nullptr || processing) {
        error = "A recording is already active.";
        return false;
    }
    auto candidate =
        ArrangeRecordingSession::create(directory, timeline.recordingConfiguration(), error);
    if (candidate == nullptr) return false;
    arrangeRecording = std::move(candidate);
    cancelled.store(false, std::memory_order_release);
    finalizationStatus.reset();
    timeline.setRecordingSink(arrangeRecording.get());
    return true;
}

void RecordingController::setFinalizationDispatcher(FinalizationDispatcher dispatcher) {
    const juce::ScopedLock guard(lock);
    finalizationDispatcher = std::move(dispatcher);
}

bool RecordingController::stop(juce::String& error) {
    std::unique_ptr<ArrangeRecordingSession> detached;
    FinalizationDispatcher dispatcher;
    {
        const juce::ScopedLock guard(lock);
        if (processing) {
            error = "The previous recording is still being processed.";
            return false;
        }
        if (pendingFinalization != nullptr) {
            error = "The previous recording is waiting for finalization.";
            return false;
        }
        if (!timeline.stopRecording(error)) return false;
        const auto captureFinalized = timeline.finalizeRecording(error);
        const auto transportStopped = timeline.stop().has_value();
        if (!captureFinalized) return false;
        if (!transportStopped) {
            error = "The realtime command queue is full.";
            return false;
        }
        timeline.clearRecordingSink();
        if (arrangeRecording == nullptr) return true;

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
    return true;
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

bool RecordingController::cancel(juce::String& error) {
    const juce::ScopedLock guard(lock);
    if (processing || pendingFinalization != nullptr) {
        error = "The previous recording is still being processed.";
        return false;
    }
    timeline.clearRecordingSink();
    if (arrangeRecording == nullptr) {
        cancelled.store(true, std::memory_order_release);
        return true;
    }
    auto cancelling = std::move(arrangeRecording);
    const auto wasCancelled = cancelling->cancel(error);
    cancelled.store(wasCancelled, std::memory_order_release);
    return wasCancelled;
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
