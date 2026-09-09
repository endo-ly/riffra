#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <optional>
#include <thread>

#include "AudioCommandDispatcher.h"
#include "MidiInputService.h"
#include "PluginEditorHost.h"
#include "RuntimeLifecycleExecutor.h"
#include "TimelineEngine.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceController.h"

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
    std::shared_ptr<PluginEditorHost> trackPluginEditor;
    juce::String trackPluginEditorTrackId;
    juce::String trackPluginEditorDeviceId;
    juce::AudioBuffer<float> comparisonRaw;
    juce::AudioBuffer<float> comparisonProcessed;
    std::atomic<bool> timelineOperationRunning{false};
    RuntimeLifecycleExecutor runtimeLifecycle;
    std::unique_ptr<AudioCommandDispatcher> commandDispatcher;
    std::atomic<bool> watchdogRunning{false};
    std::atomic<bool> midiPollRunning{false};
    std::atomic<bool> meterPushRunning{false};
    std::atomic<bool> transportPushRunning{false};
    std::thread watchdog;
    std::thread midiPollThread;
    std::thread meterPushThread;
    std::thread transportPushThread;
};

}  // namespace riffra
