#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <istream>
#include <memory>

#include "AudioStatusBuilder.h"
#include "plugins/RuntimeLifecycleExecutor.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceController.h"

namespace riffra {

class MidiInputService;
class PluginEditorHost;
class TimelineEngine;

struct CommandResult {
    bool shutdown = false;
};

/// Owns stdin command parsing and dispatches each command family.
class AudioCommandDispatcher final {
public:
    struct Context {
        juce::AudioFormatManager& formatManager;
        TimelineEngine& timelineEngine;
        AudioRenderPipeline& pipeline;
        AudioDeviceController& deviceController;
        MidiInputService& midiInputs;
        RuntimeLifecycleExecutor& runtimeLifecycle;
        std::shared_ptr<PluginEditorHost>& trackPluginEditor;
        juce::String& trackPluginEditorTrackId;
        juce::String& trackPluginEditorDeviceId;
        juce::AudioBuffer<float>& comparisonRaw;
        juce::AudioBuffer<float>& comparisonProcessed;
        std::atomic<bool>& timelineOperationRunning;
    };

    explicit AudioCommandDispatcher(Context context) noexcept : context(context) {}

    AudioCommandDispatcher(const AudioCommandDispatcher&) = delete;
    AudioCommandDispatcher& operator=(const AudioCommandDispatcher&) = delete;

    void run(std::istream& input);
    [[nodiscard]] CommandResult dispatch(const juce::var& command);

private:
    [[nodiscard]] CommandResult dispatchSafety(const juce::var& command);
    [[nodiscard]] CommandResult dispatchTransport(const juce::var& command);
    [[nodiscard]] CommandResult dispatchTimeline(const juce::var& command);
    [[nodiscard]] CommandResult dispatchTrackDevice(const juce::var& command);
    [[nodiscard]] CommandResult dispatchRecording(const juce::var& command);
    [[nodiscard]] CommandResult dispatchPreview(const juce::var& command);
    [[nodiscard]] CommandResult dispatchMidi(const juce::var& command);
    [[nodiscard]] CommandResult dispatchDevice(const juce::var& command);

    Context context;
};

}  // namespace riffra
