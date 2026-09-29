#pragma once

#include "TimelineEngine.h"
#include "contract/ExecutionGraph.h"

namespace riffra {

/// Builds a runtime timeline graph from a JSON snapshot without publishing it.
class TimelineSnapshotBuilder final {
public:
    /// Creates a builder bound to the engine state used for runtime reuse.
    explicit TimelineSnapshotBuilder(TimelineEngine& engine) noexcept;

    /// Validates and prepares a snapshot for later publication by the engine.
    [[nodiscard]] bool build(const TimelineSnapshotSpec& snapshot,
                             juce::AudioFormatManager& formats, double outputSampleRate,
                             int maximumBlockSize, std::unique_ptr<PreparedTimeline>& prepared,
                             juce::String& error);

private:
    TimelineEngine& engine;
};

}  // namespace riffra
