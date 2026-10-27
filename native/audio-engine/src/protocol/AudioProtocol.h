#pragma once

#include <JuceHeader.h>

#include "contract/SidecarMessages.h"

namespace riffra {

/// Writes an encoded response or error envelope on the ordered control lane.
void writeControlEnvelope(const juce::var& envelope);

/// Writes one event on the output lane its type requires.
///
/// `ready`, `fault`, and `recordingComplete` are ordering barriers,
/// `audioStatus` and plugin events keep only their latest value per key, and
/// meters and transport status are lossy telemetry.
void writeEvent(const SidecarEventSpec& event);

/// Writes an editor's final state on the ordered control lane.
void writeClosingPluginState(const PluginStateChangedSpec& event);

}  // namespace riffra
