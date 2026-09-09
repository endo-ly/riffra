#pragma once

#include "TimelineEngine.h"

namespace riffra {

/// Builds a runtime timeline graph from a JSON snapshot without publishing it.
class TimelineSnapshotBuilder final {
public:
    /// Creates a builder bound to the engine state used for runtime reuse.
    explicit TimelineSnapshotBuilder(TimelineEngine& engine) noexcept;

    /// Validates and prepares a snapshot for later publication by the engine.
    [[nodiscard]] bool build(const juce::var& snapshot, juce::AudioFormatManager& formats,
                             double outputSampleRate, int maximumBlockSize,
                             std::unique_ptr<TimelineEngine::PreparedTimeline>& prepared,
                             bool& monitorLiveInputState,
                             std::uint32_t& monitoringInputChannelsState,
                             bool& armedInstrumentTrackState, juce::String& error);

private:
    TimelineEngine& engine;
};

}  // namespace riffra
