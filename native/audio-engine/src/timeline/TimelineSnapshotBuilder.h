#pragma once

#include <map>
#include <optional>
#include <vector>

#include "TimelineEngine.h"
#include "contract/ExecutionGraph.h"

namespace riffra {

inline bool sameEffectTopology(const std::vector<PluginDeviceSpec>& left,
                               const std::vector<PluginDeviceSpec>& right) noexcept {
    if (left.size() != right.size()) return false;
    for (std::size_t index = 0; index < left.size(); ++index) {
        if (left[index].id != right[index].id || left[index].path != right[index].path)
            return false;
    }
    return true;
}

inline bool sameInstrumentTopology(const std::optional<InstrumentSpec>& left,
                                   const std::optional<InstrumentSpec>& right) noexcept {
    if (left.has_value() != right.has_value()) return false;
    if (!left.has_value()) return true;
    if (left->index() != right->index()) return false;
    if (const auto* leftVst3 = std::get_if<Vst3InstrumentSpec>(&*left)) {
        const auto& rightVst3 = std::get<Vst3InstrumentSpec>(*right);
        return leftVst3->id == rightVst3.id && leftVst3->path == rightVst3.path;
    }
    const auto& leftInternal = std::get<InternalInstrumentSpec>(*left);
    const auto& rightInternal = std::get<InternalInstrumentSpec>(*right);
    return leftInternal.id == rightInternal.id &&
           leftInternal.definitionJson == rightInternal.definitionJson &&
           leftInternal.definitionBaseDir == rightInternal.definitionBaseDir;
}

inline bool sameInstrumentRuntime(const std::optional<InstrumentSpec>& left,
                                  const std::optional<InstrumentSpec>& right) {
    if (left.has_value() != right.has_value()) return false;
    if (!left.has_value()) return true;
    if (left->index() != right->index()) return false;
    if (const auto* leftVst3 = std::get_if<Vst3InstrumentSpec>(&*left)) {
        const auto& rightVst3 = std::get<Vst3InstrumentSpec>(*right);
        return leftVst3->id == rightVst3.id && leftVst3->path == rightVst3.path &&
               leftVst3->state == rightVst3.state;
    }
    const auto& leftInternal = std::get<InternalInstrumentSpec>(*left);
    const auto& rightInternal = std::get<InternalInstrumentSpec>(*right);
    return leftInternal.id == rightInternal.id &&
           leftInternal.definitionJson == rightInternal.definitionJson &&
           leftInternal.definitionBaseDir == rightInternal.definitionBaseDir;
}

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
