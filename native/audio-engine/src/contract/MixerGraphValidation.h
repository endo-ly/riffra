#pragma once

#include <algorithm>
#include <set>
#include <tuple>

#include "ExecutionGraph.h"

namespace riffra {

inline bool validateMixerGraph(const ExecutionGraph& graph, juce::String& error) {
    std::set<juce::String> channelIds, deviceIds, effectDeviceIds, instrumentIds, sendIds;
    std::set<std::pair<juce::String, juce::String>> routes;
    const auto effectsValid = [&](const std::vector<PluginDeviceSpec>& effects) {
        for (const auto& effect : effects)
            if (effect.id.isEmpty() || !deviceIds.insert(effect.id).second ||
                !effectDeviceIds.insert(effect.id).second)
                return false;
        return true;
    };
    for (const auto& track : graph.tracks) {
        if (track.id.isEmpty() || !channelIds.insert(track.id).second ||
            !effectsValid(track.effects)) {
            error = "graph channel and device identities must be unique";
            return false;
        }
        if (track.instrument) {
            const auto instrumentId =
                std::visit([](const auto& instrument) { return instrument.id; }, *track.instrument);
            if (!deviceIds.insert(instrumentId).second ||
                !instrumentIds.insert(instrumentId).second) {
                error = "graph channel and device identities must be unique";
                return false;
            }
        }
    }
    for (const auto& bus : graph.mixer.buses) {
        if (bus.id.isEmpty() || !channelIds.insert(bus.id).second || !effectsValid(bus.effects)) {
            error = "graph channel and device identities must be unique";
            return false;
        }
    }
    if (!effectsValid(graph.mixer.masterEffects)) {
        error = "graph device identities must be unique";
        return false;
    }
    const auto hasBus = [&](const juce::String& id, const MixBusKindSpec kind) {
        return std::any_of(graph.mixer.buses.begin(), graph.mixer.buses.end(),
                           [&](const auto& bus) { return bus.id == id && bus.kind == kind; });
    };
    for (const auto& track : graph.tracks) {
        if (track.outputBusId && !hasBus(*track.outputBusId, MixBusKindSpec::group)) {
            error = "track output group is unavailable: " + *track.outputBusId;
            return false;
        }
    }
    for (const auto& send : graph.mixer.sends) {
        if (send.id.isEmpty() || !sendIds.insert(send.id).second ||
            !routes.emplace(send.trackId, send.returnBusId).second ||
            !std::any_of(graph.tracks.begin(), graph.tracks.end(),
                         [&](const auto& track) { return track.id == send.trackId; }) ||
            !hasBus(send.returnBusId, MixBusKindSpec::returnBus)) {
            error = "send routing cannot be executed: " + send.id;
            return false;
        }
    }
    std::set<juce::String> sidechainIds;
    std::set<std::pair<juce::String, juce::String>> sidechainRoutes;
    for (const auto& send : graph.mixer.sidechainSends) {
        if (send.id.isEmpty() || !sidechainIds.insert(send.id).second ||
            channelIds.contains(send.id) || sendIds.contains(send.id) ||
            !sidechainRoutes.emplace(send.sourceTrackId, send.targetDeviceId).second) {
            error = "sidechain routing cannot be executed: " + send.id;
            return false;
        }
        const auto source =
            std::find_if(graph.tracks.begin(), graph.tracks.end(),
                         [&](const auto& track) { return track.id == send.sourceTrackId; });
        if (source == graph.tracks.end()) {
            error = "sidechain routing cannot be executed: " + send.id;
            return false;
        }
        const auto isTrackEffect = [&](const juce::String& deviceId) {
            return std::any_of(source->effects.begin(), source->effects.end(),
                               [&](const auto& effect) { return effect.id == deviceId; });
        };
        if (instrumentIds.contains(send.targetDeviceId) ||
            !effectDeviceIds.contains(send.targetDeviceId) || isTrackEffect(send.targetDeviceId)) {
            error = "sidechain routing cannot be executed: " + send.id;
            return false;
        }
    }
    if (graph.automation.size() > 16'384) {
        error = "graph cannot contain more than 16384 automation lanes";
        return false;
    }
    std::set<std::tuple<AutomationTargetKind, juce::String, juce::String>> targets;
    for (const auto& lane : graph.automation) {
        bool resolved = true;
        switch (lane.kind) {
            case AutomationTargetKind::trackVolume:
            case AutomationTargetKind::trackPan:
                resolved = std::any_of(graph.tracks.begin(), graph.tracks.end(),
                                       [&](const auto& track) { return track.id == lane.id; });
                break;
            case AutomationTargetKind::busGain:
            case AutomationTargetKind::busPan:
                resolved = std::any_of(graph.mixer.buses.begin(), graph.mixer.buses.end(),
                                       [&](const auto& bus) { return bus.id == lane.id; });
                break;
            case AutomationTargetKind::sendLevel:
                resolved = sendIds.contains(lane.id);
                break;
            case AutomationTargetKind::sidechainSendLevel:
                resolved = sidechainIds.contains(lane.id);
                break;
            case AutomationTargetKind::deviceParameter:
                // Unavailable plugins retain canonical lanes and are diagnosed during binding.
                break;
            case AutomationTargetKind::masterGain:
                break;
        }
        if (!resolved || !targets.emplace(lane.kind, lane.id, lane.parameterId).second) {
            error = "automation target is missing or duplicated: " + lane.id;
            return false;
        }
    }
    return true;
}

}  // namespace riffra
