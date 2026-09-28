#include "ArrangementGraph.h"

#include <algorithm>

#include "MidiSourceRegistry.h"

namespace riffra {

bool ArrangementGraph::midiRouteMatches(const std::uint16_t configuredSource,
                                        const int configuredChannel, const std::uint16_t source,
                                        const int messageChannel) noexcept {
    return (configuredSource == MidiSourceRegistry::kAllSources || configuredSource == source) &&
           (configuredChannel == 0 || configuredChannel == messageChannel);
}

const float* ArrangementGraph::audioInputSource(const int configuredChannel,
                                                const float* const* physicalInputChannels,
                                                const int physicalInputChannelCount) noexcept {
    if (configuredChannel < 0 || configuredChannel >= physicalInputChannelCount ||
        physicalInputChannels == nullptr)
        return nullptr;
    return physicalInputChannels[configuredChannel];
}

bool ArrangementGraph::shouldMonitorAudioInput(const MonitoringSpec monitoring, const bool armed,
                                               const bool instrument) noexcept {
    return !instrument &&
           (monitoring == MonitoringSpec::on || (monitoring == MonitoringSpec::automatic && armed));
}

std::int64_t ArrangementGraph::compensationDelay(const std::int64_t maximumPluginDelay,
                                                 const std::int64_t trackPluginDelay) noexcept {
    return std::max<std::int64_t>(0, maximumPluginDelay - trackPluginDelay);
}

std::pair<int, int> ArrangementGraph::captureIntersection(const int chunkStart,
                                                          const int chunkSamples,
                                                          const int captureStart,
                                                          const int captureSamples) noexcept {
    const auto start = std::max(chunkStart, captureStart);
    const auto end = std::min(chunkStart + std::max(0, chunkSamples),
                              captureStart + std::max(0, captureSamples));
    return {start, std::max(start, end)};
}

}  // namespace riffra
