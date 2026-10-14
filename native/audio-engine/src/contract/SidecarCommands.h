#pragma once

#include <JuceHeader.h>

#include <array>
#include <cstdint>
#include <optional>
#include <string_view>
#include <variant>

#include "ExecutionGraph.h"
#include "audio/InstrumentPreviewSession.h"

namespace riffra {

struct StatusCommand final {};

struct SetEmergencyMuteCommand final {
    bool muted = false;
};

struct SetFeedbackProtectionCommand final {
    bool active = false;
};

struct SetEngineTransitionMuteCommand final {
    bool active = false;
};

struct PreviewMasterGainDbCommand final {
    double gainDb = 0.0;
};

struct PrepareTimelineSnapshotCommand final {
    TimelineSnapshotSpec snapshot;
};

struct CommitTimelineSnapshotCommand final {};

struct DiscardTimelineSnapshotCommand final {};

struct WaitForTimelineIdleCommand final {
    std::uint64_t timeoutMs = 0;
};

enum class TransportCommandKind { play, setStarting, stop, seek };

struct TransportCommand final {
    TransportCommandKind kind = TransportCommandKind::stop;
    /// Target position of `seek`; zero for the other kinds.
    std::uint64_t tick = 0;
};

struct SetMidiListeningCommand final {
    bool listening = false;
};

struct SetLiveMidiTargetCommand final {
    std::optional<juce::String> trackId;
};

struct SendTrackMidiCommand final {
    juce::String trackId;
    juce::MidiMessage message;
};

struct PanicTrackMidiCommand final {
    juce::String trackId;
};

struct SetChannelMixCommand final {
    EffectOwnerSpec owner;
    std::optional<double> gainDb;
    std::optional<double> pan;
};

struct SetSendLevelCommand final {
    juce::String sendId;
    double levelDb = 0.0;
};

struct SetDeviceBypassedCommand final {
    EffectOwnerSpec owner;
    juce::String deviceId;
    bool bypassed = false;
};

struct SetDeviceParameterCommand final {
    EffectOwnerSpec owner;
    juce::String deviceId;
    std::uint32_t parameterIndex = 0;
    std::optional<float> value;
    std::optional<juce::String> display;
};

enum class DeviceQuery { status, parameters, programs, pluginState };

struct GetDeviceCommand final {
    DeviceQuery query = DeviceQuery::status;
    EffectOwnerSpec owner;
    juce::String deviceId;
};

struct SetPluginStateCommand final {
    EffectOwnerSpec owner;
    juce::String deviceId;
    PluginStateSpec state;
};

struct SetDeviceProgramCommand final {
    EffectOwnerSpec owner;
    juce::String deviceId;
    std::uint32_t programIndex = 0;
};

struct OpenPluginEditorCommand final {
    juce::String projectId;
    EffectOwnerSpec owner;
    juce::String deviceId;
};

struct PreviewSampleCommand final {
    juce::String path;
    std::uint64_t startMs = 0;
    std::optional<std::uint64_t> endMs;
    double gain = 1.0;
    bool loop = false;
};

struct PreviewInstrumentCommand final {
    juce::String definitionJson;
    juce::String definitionBaseDir;
    InstrumentPreviewSpec preview;
};

struct StopPreviewCommand final {};

struct StopInstrumentPreviewCommand final {};

/// Opens a VST3's editor outside the Project, played live like its standalone application.
struct OpenPluginAuditionCommand final {
    juce::String path;
};

struct StartTakeComparisonCommand final {
    juce::String rawPath;
    juce::String processedPath;
    std::uint64_t rawStartFrame = 0;
    std::uint64_t rawEndFrame = 0;
    std::uint64_t processedStartFrame = 0;
    std::uint64_t processedEndFrame = 0;
};

enum class TakeComparisonVariantSpec { raw, processed };

struct SwitchTakeComparisonVariantCommand final {
    TakeComparisonVariantSpec variant = TakeComparisonVariantSpec::raw;
};

struct StopTakeComparisonCommand final {};

struct RecoverAudioDeviceCommand final {};

struct SetAudioDriverCommand final {
    juce::String driver;
    std::optional<juce::String> inputDevice;
    std::uint32_t inputChannel = 0;
    std::optional<juce::String> outputDevice;
    std::optional<std::uint32_t> sampleRate;
    std::optional<std::uint32_t> bufferSize;
};

struct StartArrangeRecordingCommand final {
    juce::String directory;
    std::uint8_t countInBeats = 0;
};

struct StopArrangeRecordingCommand final {};

using SidecarCommandSpec = std::variant<
    StatusCommand, SetEmergencyMuteCommand, SetFeedbackProtectionCommand,
    SetEngineTransitionMuteCommand, PreviewMasterGainDbCommand, PrepareTimelineSnapshotCommand,
    CommitTimelineSnapshotCommand, DiscardTimelineSnapshotCommand, WaitForTimelineIdleCommand,
    TransportCommand, SetMidiListeningCommand, SetLiveMidiTargetCommand, SendTrackMidiCommand,
    PanicTrackMidiCommand, SetChannelMixCommand, SetSendLevelCommand, SetDeviceBypassedCommand,
    SetDeviceParameterCommand, GetDeviceCommand, SetPluginStateCommand, SetDeviceProgramCommand,
    OpenPluginEditorCommand, PreviewSampleCommand, PreviewInstrumentCommand, StopPreviewCommand,
    StopInstrumentPreviewCommand, OpenPluginAuditionCommand, StartTakeComparisonCommand,
    SwitchTakeComparisonVariantCommand, StopTakeComparisonCommand, RecoverAudioDeviceCommand,
    SetAudioDriverCommand, StartArrangeRecordingCommand, StopArrangeRecordingCommand>;

/// One decoded stdin line of the realtime sidecar.
struct SidecarRequestSpec final {
    std::uint64_t requestId = 0;
    SidecarCommandSpec command;
};

/// Every command `type` accepted on stdin.
inline constexpr std::array<std::string_view, 41> kSidecarCommandTypes{
    "status",
    "setEmergencyMute",
    "setFeedbackProtection",
    "setEngineTransitionMute",
    "previewMasterGainDb",
    "prepareTimelineSnapshot",
    "commitTimelineSnapshot",
    "discardTimelineSnapshot",
    "waitForTimelineIdle",
    "playTimeline",
    "setTransportStarting",
    "stopTimeline",
    "seekTimeline",
    "enableMidiListening",
    "disableMidiListening",
    "setLiveMidiTarget",
    "sendTrackMidi",
    "panicTrackMidi",
    "setChannelMix",
    "setSendLevel",
    "setDeviceBypassed",
    "setDeviceParameter",
    "getDeviceStatus",
    "getDeviceParameters",
    "getDevicePrograms",
    "getPluginState",
    "setPluginState",
    "setDeviceProgram",
    "openPluginEditor",
    "previewSample",
    "previewInstrument",
    "stopPreview",
    "stopInstrumentPreview",
    "openPluginAudition",
    "startTakeComparison",
    "switchTakeComparisonVariant",
    "stopTakeComparison",
    "recoverAudioDevice",
    "setAudioDriver",
    "startArrangeRecording",
    "stopArrangeRecording",
};

/// Reads the request id of an envelope whose command may be invalid.
[[nodiscard]] std::optional<std::uint64_t> readSidecarRequestId(const juce::var& envelope);

/// Decodes one envelope, rejecting unknown, missing, mistyped, and out-of-range keys.
[[nodiscard]] bool decodeSidecarRequest(const juce::var& envelope, SidecarRequestSpec& output,
                                        juce::String& error);

}  // namespace riffra
