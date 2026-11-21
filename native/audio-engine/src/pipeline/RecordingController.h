#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <thread>

#include "contract/SidecarMessages.h"
#include "recording/ArrangeRecordingSession.h"
#include "timeline/RealtimeCommand.h"

namespace riffra {

class TimelineEngine;

/// Owns arrange-recording control state, finalization hand-off, and stop completion.
class RecordingController final {
public:
    using FinalizationDispatcher =
        std::function<void(std::unique_ptr<ArrangeRecordingSession>, const juce::String&)>;
    /// Reports an asynchronously applied stop on the stop worker. The handler
    /// must not call setStopCompletion; doing so deadlocks while the controller
    /// waits for this invocation to finish.
    using StopCompletion = std::function<void(bool completed, const juce::String& error)>;

    explicit RecordingController(TimelineEngine& timeline) noexcept;
    ~RecordingController();

    RecordingController(const RecordingController&) = delete;
    RecordingController& operator=(const RecordingController&) = delete;

    // Control thread only.
    RealtimeRequest start(const juce::File& directory, int countInBeats, juce::String& error);
    /// Accepts one asynchronous stop. The worker applies the stop and then calls
    /// the completion handler. `cancelCountIn` reports whether the stop cancels
    /// a count-in rather than a running capture.
    RealtimeRequest requestStopAsync(bool& cancelCountIn, juce::String& error);
    RealtimeRequest stop(juce::String& error);
    void setFinalizationDispatcher(FinalizationDispatcher dispatcher);
    /// Replaces the stop completion handler. Returns only after a previously
    /// running handler has finished, so the caller may destroy its state after
    /// this returns. Must not be called from a stop completion handler.
    void setStopCompletion(StopCompletion completion);
    [[nodiscard]] bool waitForStopCompletion(std::chrono::milliseconds timeout);
    void completeProcessing(const ArrangeRecordingSummary& summary, const juce::String& error);
    [[nodiscard]] RecordingStatusSpec status() const;

private:
    struct PendingRecordingStop final {
        std::uint64_t commandSequence = 0;
        bool cancelCountIn = false;
    };

    RealtimeRequest beginStop(PendingRecordingStop& pending, juce::String& error);
    RealtimeRequest applyStop(const PendingRecordingStop& pending, juce::String& error);
    void finishStop();
    void stopWorkerLoop();

    TimelineEngine& timeline;
    mutable juce::CriticalSection lock;
    std::unique_ptr<ArrangeRecordingSession> arrangeRecording;
    FinalizationDispatcher finalizationDispatcher;
    std::optional<RecordingStatusSpec> finalizationStatus;
    bool processing = false;
    std::atomic<bool> cancelled{false};

    std::mutex stopMutex;
    std::condition_variable stopWake;
    std::condition_variable stopFinished;
    std::condition_variable stopCompletionFinished;
    std::optional<PendingRecordingStop> pendingStop;
    StopCompletion stopCompletion;
    bool stopCompletionRunning = false;
    bool stopInProgress = false;
    bool stopWorkerStopping = false;
    std::thread stopWorker;
};

}  // namespace riffra
