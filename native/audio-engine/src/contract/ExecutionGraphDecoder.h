#pragma once

#include <JuceHeader.h>

#include "ExecutionGraph.h"

namespace riffra {

[[nodiscard]] bool decodeTimelineSnapshot(const juce::var& value, TimelineSnapshotSpec& output,
                                          juce::String& error);
[[nodiscard]] bool decodeOfflineRenderRequest(const juce::var& value,
                                              OfflineRenderRequestSpec& output,
                                              juce::String& error);
[[nodiscard]] bool decodePluginState(const juce::var& value, PluginStateSpec& output,
                                     juce::String& error);

}  // namespace riffra
