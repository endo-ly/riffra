#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <functional>

namespace riffra {

enum class LoadPhase { queued, creating, restoring, preparing, deviceReady, routing, prepared };

inline const char* loadPhaseName(const LoadPhase phase) noexcept {
    switch (phase) {
        case LoadPhase::queued:
            return "queued";
        case LoadPhase::creating:
            return "creating";
        case LoadPhase::restoring:
            return "restoring";
        case LoadPhase::preparing:
            return "preparing";
        case LoadPhase::deviceReady:
            return "deviceReady";
        case LoadPhase::routing:
            return "routing";
        case LoadPhase::prepared:
            return "prepared";
    }
    return "queued";
}

/// Reports completed preparation boundaries, never a periodic heartbeat.
struct LoadProgressSpec final {
    LoadPhase phase = LoadPhase::queued;
    juce::String ownerId;
    juce::String deviceId;
    juce::String pluginPath;
    std::uint32_t completedDevices = 0;
    std::uint32_t totalDevices = 0;
};

using LoadObserver = std::function<void(const LoadProgressSpec&)>;
using PluginLoadObserver = std::function<void(LoadPhase)>;
using DeviceLoadObserver = std::function<void(const juce::String&, const juce::String&, LoadPhase)>;

}  // namespace riffra
