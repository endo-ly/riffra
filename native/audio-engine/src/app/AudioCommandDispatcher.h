#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <istream>
#include <memory>
#include <mutex>
#include <optional>
#include <thread>

#include "AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "contract/SidecarCommands.h"
#include "device/AudioDeviceController.h"
#include "plugins/RuntimeLifecycleExecutor.h"
#include "protocol/CommandResponder.h"
#include "timeline/RealtimeCommand.h"

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
        std::shared_ptr<PluginEditorHost>& auditionEditor;
        juce::AudioBuffer<float>& comparisonRaw;
        juce::AudioBuffer<float>& comparisonProcessed;
        std::atomic<bool>& timelineOperationRunning;
    };

    explicit AudioCommandDispatcher(Context context);
    ~AudioCommandDispatcher();

    AudioCommandDispatcher(const AudioCommandDispatcher&) = delete;
    AudioCommandDispatcher& operator=(const AudioCommandDispatcher&) = delete;

    /// Reads and dispatches command lines until standard input closes.
    void run(std::istream& input);
    void dispatch(const SidecarRequestSpec& request, CommandResponder responder);
    [[nodiscard]] bool waitForBackgroundWork(std::chrono::milliseconds timeout);
    /// Closes the auditioned plug-in's editor and destroys the plug-in.
    /// Lifecycle thread only.
    void closePluginAudition();

private:
    /// Status reported by commands whose response is the full audio status.
    [[nodiscard]] AudioStatusSpec currentStatus() const;
    /// Rejects a command that must not overlap an Arrangement Graph lifecycle task.
    [[nodiscard]] bool rejectWhileTimelineBusy(CommandResponder& responder,
                                               const juce::String& message);
    /// Fails the request unless the timeline accepted it. A full realtime
    /// command queue is reported as the retryable `realtimeQueueFull`.
    [[nodiscard]] static bool rejectUnlessAccepted(CommandResponder& responder,
                                                   RealtimeRequest request,
                                                   const juce::String& kind,
                                                   const juce::String& error,
                                                   const juce::String& operation);
    /// Fails the request when a realtime command could not be queued.
    [[nodiscard]] static bool rejectUnlessQueued(CommandResponder& responder, bool queued,
                                                 const juce::String& operation);
    [[nodiscard]] bool reserveRecordingStop(CommandResponder& responder);
    void releaseRecordingStopReservation();
    void queueRecordingStop(const PendingRecordingStop& pending);
    void recordingStopWorkerLoop();

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
    void handle(const OpenPluginAuditionCommand& command, CommandResponder responder);
    /// Shows the editor of the plug-in at `path`, loading it unless it is
    /// already the auditioned one. Lifecycle thread only.
    [[nodiscard]] bool openPluginAudition(const juce::String& path, juce::String& error);
    void handle(const StartTakeComparisonCommand& command, CommandResponder responder);
    void handle(const SwitchTakeComparisonVariantCommand& command, CommandResponder responder);
    void handle(const StopTakeComparisonCommand&, CommandResponder responder);
    void handle(const RecoverAudioDeviceCommand&, CommandResponder responder);
    void handle(const SetAudioDriverCommand& command, CommandResponder responder);
    void handle(const StartArrangeRecordingCommand& command, CommandResponder responder);
    void handle(const StopArrangeRecordingCommand&, CommandResponder responder);

    Context context;
    std::mutex recordingStopMutex;
    std::condition_variable recordingStopWake;
    std::condition_variable recordingStopFinished;
    std::optional<PendingRecordingStop> pendingRecordingStop;
    bool recordingStopScheduled = false;
    bool recordingStopWorkerStopping = false;
    std::thread recordingStopWorker;
};

}  // namespace riffra
