#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <optional>
#include <thread>

#include "AudioCommandDispatcher.h"
#include "RuntimeLifecycleExecutor.h"
#include "device/AudioDeviceController.h"
#include "device/MidiInputService.h"
#include "pipeline/AudioRenderPipeline.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

/// Owns the --serve runtime and coordinates its existing thread boundaries.
class AudioEngine final {
public:
    AudioEngine();
    ~AudioEngine();

    AudioEngine(const AudioEngine&) = delete;
    AudioEngine& operator=(const AudioEngine&) = delete;

    [[nodiscard]] int serve(std::optional<std::uint32_t> parentPid,
                            const AudioConfiguration& startupConfiguration);

private:
    juce::AudioFormatManager formatManager;
    TimelineEngine timelineEngine;
    AudioRenderPipeline pipeline;
    AudioDeviceController deviceController;
    MidiInputService midiInputs;
    RuntimeLifecycleExecutor runtimeLifecycle;
    std::unique_ptr<AudioCommandDispatcher> commandDispatcher;
    std::atomic<bool> watchdogRunning{false};
    std::atomic<bool> midiPollRunning{false};
    std::atomic<bool> telemetryPushRunning{false};
    std::thread watchdog;
    std::thread midiPollThread;
    std::thread telemetryPushThread;
};

}  // namespace riffra
