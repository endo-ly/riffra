#include "RecordingController.h"

#include <utility>

#include "recording/ArrangeRecordingSession.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

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
    finalizationStatus = juce::var{};
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
        timeline.stopRecording();
        const auto captureFinalized = timeline.finalizeRecording(error);
        timeline.stop();
        if (!captureFinalized) return false;
        timeline.clearRecordingSink();
        if (arrangeRecording == nullptr) return true;

        detached = std::move(arrangeRecording);
        processing = true;
        finalizationStatus = detached->status();
        if (auto* statusObject = finalizationStatus.getDynamicObject()) {
            statusObject->setProperty("active", false);
            statusObject->setProperty("processing", true);
        }
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

void RecordingController::completeProcessing(const juce::var& status, const juce::String& error) {
    const juce::ScopedLock guard(lock);
    finalizationStatus = status;
    if (auto* result = finalizationStatus.getDynamicObject()) {
        result->setProperty("active", false);
        result->setProperty("processing", false);
        if (error.isNotEmpty()) result->setProperty("error", error);
    }
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

juce::var RecordingController::status() const {
    const juce::ScopedLock guard(lock);
    if (arrangeRecording != nullptr) {
        auto result = arrangeRecording->status();
        if (auto* statusObject = result.getDynamicObject())
            statusObject->setProperty("processing", false);
        return result;
    }
    if (finalizationStatus.isObject()) return finalizationStatus;
    auto* result = new juce::DynamicObject();
    result->setProperty("active", false);
    result->setProperty("processing", false);
    result->setProperty("cancelled", cancelled.load(std::memory_order_acquire));
    return juce::var(result);
}

}  // namespace riffra
