#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <memory>

#include "contract/ExecutionGraph.h"

namespace riffra {

class TimelineEngine;

/// Renders one timeline range of an execution graph to a WAV file.
///
/// Some VST3 plugins complete their preparation through the message thread, so rendering is
/// split into `prepare()` on the message thread and `render()` on a separate thread while the
/// message thread keeps dispatching.
class OfflineRenderer final {
public:
    struct Result final {
        std::uint64_t frames = 0;
        double sampleRate = 0.0;
    };

    ~OfflineRenderer();

    OfflineRenderer(const OfflineRenderer&) = delete;
    OfflineRenderer& operator=(const OfflineRenderer&) = delete;

    /// Validates the request and instantiates its devices for offline processing. Message
    /// thread only. Returns null with `error` set when the request cannot be rendered.
    [[nodiscard]] static std::unique_ptr<OfflineRenderer> prepare(
        const OfflineRenderRequestSpec& request, juce::AudioFormatManager& formats,
        juce::String& error);

    /// Writes the prepared range. A graph hosting VST3 plugins is first processed in silence
    /// at realtime pace, which requires the message thread to keep dispatching meanwhile.
    [[nodiscard]] bool render(juce::AudioFormatManager& formats, Result& result,
                              juce::String& error);

private:
    friend class TimelineEngineTestPeer;

    struct Plan final {
        juce::File destination;
        double sampleRate = 0.0;
        int blockSize = 0;
        std::int64_t startSample = 0;
        std::int64_t endSample = 0;
        float masterGain = 1.0f;
        bool normalize = false;
        bool hostsPlugins = false;
    };

    OfflineRenderer(Plan renderPlan, std::unique_ptr<TimelineEngine> timelineEngine) noexcept;

    const Plan plan;
    const std::unique_ptr<TimelineEngine> engine;
};

}  // namespace riffra
