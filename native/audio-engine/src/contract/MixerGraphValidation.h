#pragma once

#include <algorithm>
#include <set>

#include "ExecutionGraph.h"

namespace riffra {

inline bool validateMixerGraph(const ExecutionGraph& graph, juce::String& error) {
    std::set<juce::String> channelIds, deviceIds, sendIds;
    std::set<std::pair<juce::String, juce::String>> routes;
    const auto effectsValid = [&](const std::vector<PluginDeviceSpec>& effects) {
        for (const auto& effect : effects)
            if (effect.id.isEmpty() || !deviceIds.insert(effect.id).second) return false;
        return true;
    };
    for (const auto& track : graph.tracks) {
        if (track.id.isEmpty() || !channelIds.insert(track.id).second ||
            !effectsValid(track.effects) ||
            (track.instrument &&
             !deviceIds
                  .insert(std::visit([](const auto& instrument) { return instrument.id; },
                                     *track.instrument))
                  .second)) {
            error = "graph channel and device identities must be unique";
            return false;
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
    return true;
}

}  // namespace riffra
