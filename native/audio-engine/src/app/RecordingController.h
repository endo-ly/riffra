#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>

#include "contract/SidecarMessages.h"
#include "recording/ArrangeRecordingSession.h"
#include "timeline/RealtimeCommand.h"

namespace riffra {

class TimelineEngine;

/// Realtime stop command state handed from the dispatcher to its completion worker.
struct PendingRecordingStop final {
    std::uint64_t commandSequence = 0;
    bool cancelCountIn = false;
};

/// Owns arrange-recording control state and finalization hand-off.
class RecordingController final {
public:
    using FinalizationDispatcher =
        std::function<void(std::unique_ptr<ArrangeRecordingSession>, const juce::String&)>;

    explicit RecordingController(TimelineEngine& timeline) noexcept;
    ~RecordingController();

    RecordingController(const RecordingController&) = delete;
    RecordingController& operator=(const RecordingController&) = delete;

    // Control thread only.
    RealtimeRequest start(const juce::File& directory, int countInBeats, juce::String& error);
    RealtimeRequest requestStop(PendingRecordingStop& pending, juce::String& error);
    RealtimeRequest completeStop(const PendingRecordingStop& pending, juce::String& error);
    RealtimeRequest stop(juce::String& error);
    void setFinalizationDispatcher(FinalizationDispatcher dispatcher);
    std::unique_ptr<ArrangeRecordingSession> takePendingFinalization() noexcept;
    void completeProcessing(const ArrangeRecordingSummary& summary, const juce::String& error);
    [[nodiscard]] RecordingStatusSpec status() const;

private:
    TimelineEngine& timeline;
    mutable juce::CriticalSection lock;
    std::unique_ptr<ArrangeRecordingSession> arrangeRecording;
    std::unique_ptr<ArrangeRecordingSession> pendingFinalization;
    FinalizationDispatcher finalizationDispatcher;
    std::optional<RecordingStatusSpec> finalizationStatus;
    bool processing = false;
    bool stopInProgress = false;
    std::atomic<bool> cancelled{false};
};

}  // namespace riffra
