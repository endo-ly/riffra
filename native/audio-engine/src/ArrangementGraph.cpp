#include "ArrangementGraph.h"

#include <algorithm>

namespace riffra {

bool ArrangementGraph::midiRouteMatches(
    const juce::String& configuredDeviceId,
    const int configuredChannel,
    const juce::String& sourceDeviceId,
    const int messageChannel) noexcept {
    return (configuredDeviceId.isEmpty() || configuredDeviceId == sourceDeviceId)
        && (configuredChannel == 0 || configuredChannel == messageChannel);
}

const float* ArrangementGraph::audioInputSource(
    const int configuredChannel,
    const float* const* physicalInputChannels,
    const int physicalInputChannelCount) noexcept {
    if (configuredChannel < 0 || configuredChannel >= physicalInputChannelCount
        || physicalInputChannels == nullptr)
        return nullptr;
    return physicalInputChannels[configuredChannel];
}

bool ArrangementGraph::shouldMonitorAudioInput(
    const juce::String& monitoring,
    const bool armed,
    const bool instrument) noexcept {
    return !instrument && (monitoring == "on" || (monitoring == "auto" && armed));
}

std::int64_t ArrangementGraph::compensationDelay(
    const std::int64_t maximumPluginDelay,
    const std::int64_t trackPluginDelay) noexcept {
    return std::max<std::int64_t>(0, maximumPluginDelay - trackPluginDelay);
}

std::pair<int, int> ArrangementGraph::captureIntersection(
    const int chunkStart,
    const int chunkSamples,
    const int captureStart,
    const int captureSamples) noexcept {
    const auto start = std::max(chunkStart, captureStart);
    const auto end = std::min(
        chunkStart + std::max(0, chunkSamples),
        captureStart + std::max(0, captureSamples));
    return { start, std::max(start, end) };
}

float ArrangementGraph::automationValueAt(
    const std::vector<AutomationPoint>& points,
    const std::int64_t sample,
    const float fallback) noexcept {
    if (points.empty())
        return fallback;
    const auto right = std::upper_bound(
        points.begin(),
        points.end(),
        sample,
        [](const std::int64_t position, const AutomationPoint& point) {
            return position < point.sample;
        });
    if (right == points.begin())
        return right->value;
    if (right == points.end())
        return points.back().value;
    const auto& left = *(right - 1);
    const auto distance = right->sample - left.sample;
    if (distance <= 0)
        return right->value;
    const auto amount = static_cast<float>(sample - left.sample)
        / static_cast<float>(distance);
    return left.value + (right->value - left.value) * amount;
}

} // namespace riffra
