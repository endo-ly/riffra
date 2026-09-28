#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <istream>
#include <memory>

#include "AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "contract/SidecarCommands.h"
#include "device/AudioDeviceController.h"
#include "plugins/RuntimeLifecycleExecutor.h"
#include "protocol/CommandResponder.h"

namespace riffra {

class MidiInputService;
class PluginEditorHost;
class TimelineEngine;

/// Owns stdin command decoding and dispatches each typed command.
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

    /// Reads and dispatches command lines until standard input closes.
    void run(std::istream& input);
    void dispatch(const SidecarRequestSpec& request, CommandResponder responder);

private:
    /// Status reported by commands whose response is the full audio status.
    [[nodiscard]] AudioStatusSpec currentStatus() const;
    /// Rejects a command that must not overlap an Arrangement Graph lifecycle task.
    [[nodiscard]] bool rejectWhileTimelineBusy(CommandResponder& responder,
                                               const juce::String& message);

    void handle(const StatusCommand&, CommandResponder responder);
    void handle(const SetEmergencyMuteCommand& command, CommandResponder responder);
    void handle(const SetFeedbackProtectionCommand& command, CommandResponder responder);
    void handle(const SetEngineTransitionMuteCommand& command, CommandResponder responder);
    void handle(const PreviewMasterGainDbCommand& command, CommandResponder responder);
    void handle(const PrepareTimelineSnapshotCommand& command, CommandResponder responder);
    void handle(const CommitTimelineSnapshotCommand&, CommandResponder responder);
    void handle(const DiscardTimelineSnapshotCommand&, CommandResponder responder);
    void handle(const WaitForTimelineIdleCommand& command, CommandResponder responder);
    void handle(const TransportCommand& command, CommandResponder responder);
    void handle(const SetMidiListeningCommand& command, CommandResponder responder);
    void handle(const SetLiveMidiTargetCommand& command, CommandResponder responder);
    void handle(const SendTrackMidiCommand& command, CommandResponder responder);
    void handle(const PanicTrackMidiCommand& command, CommandResponder responder);
    void handle(const SetTrackMixCommand& command, CommandResponder responder);
    void handle(const SetTrackDeviceBypassedCommand& command, CommandResponder responder);
    void handle(const SetTrackDeviceParameterCommand& command, CommandResponder responder);
    void handle(const GetTrackDeviceCommand& command, CommandResponder responder);
    void handle(const SetTrackPluginStateCommand& command, CommandResponder responder);
    void handle(const SetTrackDeviceProgramCommand& command, CommandResponder responder);
    void handle(const OpenTrackPluginEditorCommand& command, CommandResponder responder);
    void handle(const PreviewSampleCommand& command, CommandResponder responder);
    void handle(const PreviewInstrumentCommand& command, CommandResponder responder);
    void handle(const StopPreviewCommand&, CommandResponder responder);
    void handle(const StopInstrumentPreviewCommand&, CommandResponder responder);
    void handle(const StartTakeComparisonCommand& command, CommandResponder responder);
    void handle(const SwitchTakeComparisonVariantCommand& command, CommandResponder responder);
    void handle(const StopTakeComparisonCommand&, CommandResponder responder);
    void handle(const RecoverAudioDeviceCommand&, CommandResponder responder);
    void handle(const SetAudioDriverCommand& command, CommandResponder responder);
    void handle(const StartArrangeRecordingCommand& command, CommandResponder responder);
    void handle(const StopArrangeRecordingCommand&, CommandResponder responder);

    Context context;
};

}  // namespace riffra
