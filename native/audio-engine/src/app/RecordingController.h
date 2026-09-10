#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <functional>
#include <memory>

#include "recording/ArrangeRecordingSession.h"

namespace riffra {

class TimelineEngine;

/// Owns arrange-recording control state and finalization hand-off.
class RecordingController final {
public:
    using FinalizationDispatcher = std::function<void(std::unique_ptr<ArrangeRecordingSession>)>;

    explicit RecordingController(TimelineEngine& timeline) noexcept;
    ~RecordingController();

    RecordingController(const RecordingController&) = delete;
    RecordingController& operator=(const RecordingController&) = delete;

    // Control thread only.
    bool start(const juce::File& directory, juce::String& error);
    bool stop(juce::String& error);
    bool cancel(juce::String& error);
    void setFinalizationDispatcher(FinalizationDispatcher dispatcher);
    std::unique_ptr<ArrangeRecordingSession> takePendingFinalization() noexcept;
    void completeProcessing(const juce::var& status, const juce::String& error);
    [[nodiscard]] juce::var status() const;

private:
    TimelineEngine& timeline;
    mutable juce::CriticalSection lock;
    std::unique_ptr<ArrangeRecordingSession> arrangeRecording;
    std::unique_ptr<ArrangeRecordingSession> pendingFinalization;
    FinalizationDispatcher finalizationDispatcher;
    juce::var finalizationStatus;
    bool processing = false;
    std::atomic<bool> cancelled{false};
};

}  // namespace riffra
