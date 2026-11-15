#pragma once

#include <map>
#include <vector>

#include "TimelineEngine.h"
#include "contract/ExecutionGraph.h"

namespace riffra {

/// Builds a runtime timeline graph from a JSON snapshot without publishing it.
class TimelineSnapshotBuilder final {
public:
    /// Creates a builder bound to the engine state used for runtime reuse.
    explicit TimelineSnapshotBuilder(TimelineEngine& engine, LoadObserver observer = {});

    /// Validates and prepares a snapshot for later publication by the engine.
    [[nodiscard]] bool build(const TimelineSnapshotSpec& snapshot,
                             juce::AudioFormatManager& formats, double outputSampleRate,
                             int maximumBlockSize, std::unique_ptr<PreparedTimeline>& prepared,
                             juce::String& error);

private:
    bool prepareMixer(
        const ExecutionGraph& graph, PreparedTimeline& prepared, PluginProcessingMode mode,
        const std::map<juce::String, std::vector<const SidechainSendSpec*>>& sendsByTargetDevice,
        juce::String& error);
    PluginLoadObserver pluginObserver(const juce::String& owner, const juce::String& device,
                                      const juce::String& path);
    DeviceLoadObserver deviceObserver(const juce::String& owner);
    void report(const juce::String& owner, const juce::String& device, const juce::String& path,
                LoadPhase phase);
    TimelineEngine& engine;
    LoadObserver observer;
    LoadProgressSpec progress;
};

}  // namespace riffra
