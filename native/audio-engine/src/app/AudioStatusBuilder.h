#pragma once

#include <JuceHeader.h>

#include "contract/SidecarMessages.h"

namespace riffra {

class AudioRenderPipeline;
class MidiMonitor;
class TimelineEngine;

/// Builds the host-level audio status, meter, and transport projections.
class AudioStatusBuilder final {
public:
    // Control/telemetry thread only. This method may service deferred graph
    // cleanup before taking the timeline projection.
    [[nodiscard]] static AudioStatusSpec currentStatus(juce::AudioDeviceManager& manager,
                                                       const AudioRenderPipeline& pipeline,
                                                       const MidiMonitor& midi,
                                                       TimelineEngine& timeline);
    [[nodiscard]] static AudioMetersSpec currentMeters(const AudioRenderPipeline& pipeline,
                                                       TimelineEngine& timeline);
    [[nodiscard]] static TransportStatusSpec currentTransport(const TimelineEngine& timeline);
};

}  // namespace riffra
