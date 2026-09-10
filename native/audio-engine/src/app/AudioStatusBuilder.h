#pragma once

#include <JuceHeader.h>

namespace riffra {

class AudioRenderPipeline;
class MidiMonitor;
class TimelineEngine;

/// Builds the host-level audio status and meter projections.
class AudioStatusBuilder final {
public:
    // Control/telemetry thread only. This method may service deferred graph
    // cleanup before taking the timeline projection.
    [[nodiscard]] static juce::var currentStatus(juce::AudioDeviceManager& manager,
                                                 const AudioRenderPipeline& pipeline,
                                                 const MidiMonitor* midi = nullptr,
                                                 const juce::String& message = {},
                                                 TimelineEngine* timeline = nullptr);
    [[nodiscard]] static juce::var currentMeters(const AudioRenderPipeline& pipeline);
};

}  // namespace riffra
