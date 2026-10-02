#pragma once

#include <JuceHeader.h>

#include <array>
#include <cstdint>
#include <optional>
#include <string_view>
#include <variant>
#include <vector>

#include "ExecutionGraph.h"

namespace riffra {

/// Version announced by `ready` and required by every `riffra-render` request.
inline constexpr std::uint32_t kSidecarProtocolVersion = 3;

enum class AudioStateSpec { ready, muted, faulted };
enum class RecoveryStatusSpec { clean, partial };
enum class TransportStateSpec { stopped, starting, playing, faulted };
enum class RecordingPhaseSpec { idle, countingIn, recording, stopping };

struct AudioChannelSpec final {
    std::uint32_t index = 0;
    juce::String name;
};

struct MidiDeviceSpec final {
    juce::String id;
    juce::String name;
};

struct RecordingStatusSpec final {
    bool active = false;
    bool processing = false;
    bool cancelled = false;
    std::optional<juce::String> directory;
    std::optional<double> sampleRate;
    std::uint64_t samplesWritten = 0;
    std::uint64_t droppedMidiEvents = 0;
    std::uint64_t droppedBlocks = 0;
    std::uint64_t missingSamples = 0;
    std::uint64_t rawAttemptedSamples = 0;
    std::uint64_t processedAttemptedSamples = 0;
    std::uint64_t rawDroppedBlocks = 0;
    std::uint64_t processedDroppedBlocks = 0;
    std::uint64_t rawMissingSamples = 0;
    std::uint64_t processedMissingSamples = 0;
    std::optional<std::uint64_t> rawDropoutStartSample;
    std::optional<std::uint64_t> rawDropoutEndSample;
    std::optional<std::uint64_t> processedDropoutStartSample;
    std::optional<std::uint64_t> processedDropoutEndSample;
    RecoveryStatusSpec recoveryStatus = RecoveryStatusSpec::clean;
    std::optional<juce::String> error;
};

struct InstrumentFaultSpec final {
    juce::String trackId;
    juce::String instrumentType;
    std::uint32_t faultCode = 0;
    std::uint64_t droppedMidiEvents = 0;
};

struct CallbackWindowSpec final {
    std::uint32_t durationMs = 1000;
    std::uint32_t callbackCount = 0;
    std::uint32_t overruns = 0;
    std::uint32_t averageCallbackDurationUs = 0;
    std::uint32_t maximumCallbackDurationUs = 0;
};

struct RealtimeDiagnosticsSpec final {
    std::uint64_t callbackCount = 0;
    std::uint64_t callbackOverruns = 0;
    CallbackWindowSpec window;
};

struct TrackLoadSpec final {
    juce::String trackId;
    std::uint32_t averageProcessingUs = 0;
    std::uint32_t maximumProcessingUs = 0;
};

struct AudioDiagnosticsSpec final {
    RealtimeDiagnosticsSpec realtime;
    std::vector<TrackLoadSpec> trackLoads;
    double preLimiterPeak = 0.0;
    double limiterGainReductionDb = 0.0;
    std::uint64_t hardClipSamples = 0;
    std::uint64_t liveMidiDrops = 0;
    std::uint64_t graphRevision = 0;
    std::uint64_t graphPublishCount = 0;
    std::uint64_t trackCount = 0;
    std::uint64_t instrumentRuntimeCount = 0;
    std::uint64_t pluginCount = 0;
    std::uint64_t maximumLatencySamples = 0;
    std::vector<InstrumentFaultSpec> instrumentFaults;
};

struct AudioStatusSpec final {
    AudioStateSpec state = AudioStateSpec::ready;
    juce::String message;
    std::optional<juce::String> driver;
    std::optional<juce::String> inputDevice;
    std::optional<std::uint32_t> inputChannel;
    std::vector<AudioChannelSpec> inputChannels;
    std::vector<std::uint32_t> activeInputChannels;
    std::optional<juce::String> outputDevice;
    std::vector<AudioChannelSpec> outputChannels;
    std::vector<std::uint32_t> activeOutputChannels;
    std::optional<double> sampleRate;
    std::optional<std::uint32_t> bufferSize;
    std::optional<double> roundTripMs;
    std::optional<std::uint64_t> timelineTick;
    RecordingStatusSpec recording;
    std::vector<MidiDeviceSpec> midiInputs;
    std::vector<MidiDeviceSpec> midiOutputs;
    bool midiInputActive = false;
    std::uint64_t midiMessages = 0;
    std::optional<std::uint8_t> lastMidiNote;
    double inputPeak = 0.0;
    double outputPeak = 0.0;
    std::uint64_t invalidSamples = 0;
    bool feedbackSuspected = false;
    bool previewing = false;
    bool instrumentPreviewing = false;
    std::uint32_t muteReasons = 0;
    AudioDiagnosticsSpec diagnostics;
};

struct TrackMeterSpec final {
    juce::String trackId;
    double peakLeft = 0.0;
    double peakRight = 0.0;
    double rmsLeft = 0.0;
    double rmsRight = 0.0;
};

struct AudioMetersSpec final {
    std::optional<juce::String> projectId;
    double inputPeak = 0.0;
    double outputPeak = 0.0;
    double outputPeakLeft = 0.0;
    double outputPeakRight = 0.0;
    std::uint64_t invalidSamples = 0;
    double preLimiterPeak = 0.0;
    double limiterGainReductionDb = 0.0;
    std::uint64_t hardClipSamples = 0;
    std::uint32_t muteReasons = 0;
    bool feedbackSuspected = false;
    bool previewing = false;
    bool instrumentPreviewing = false;
    std::vector<TrackMeterSpec> trackMeters;
};

struct TransportStatusSpec final {
    TransportStateSpec state = TransportStateSpec::stopped;
    std::optional<std::uint64_t> revision;
    std::uint64_t timelineTick = 0;
    std::int64_t timelineSample = 0;
    std::uint64_t audioClockSample = 0;
    std::optional<double> sampleRate;
    std::uint64_t appliedCommandSequence = 0;
    RecordingPhaseSpec recordingPhase = RecordingPhaseSpec::idle;
    std::uint64_t recordingStartTick = 0;
    std::uint32_t recordingPassOrdinal = 0;
    std::vector<juce::String> armedTrackIds;
    std::vector<InstrumentFaultSpec> instrumentFaults;
    std::uint64_t clockGeneration = 0;
    std::uint64_t discontinuity = 0;
};

/// Completes `prepareTimelineSnapshot`, `commitTimelineSnapshot` and
/// `discardTimelineSnapshot`.
struct TimelineAckSpec final {};
/// Completes `waitForTimelineIdle`.
struct TimelineIdleAckSpec final {};
/// Completes `sendTrackMidi` and `panicTrackMidi`.
struct MidiAckSpec final {};
/// Completes `setTrackMix`.
struct TrackMixAckSpec final {};
/// Completes the Track Device commands that change a device or open its editor.
struct TrackDeviceAckSpec final {};

/// A transport command accepted by the realtime command queue.
struct TransportAcceptedSpec final {
    std::uint64_t commandSequence = 0;
};

struct TrackDeviceCapabilitiesSpec final {
    bool parameters = false;
    bool state = false;
    bool presets = false;
    bool editor = false;
};

struct TrackDeviceStatusSpec final {
    juce::String name;
    bool bypassed = false;
    std::uint32_t parameterCount = 0;
    TrackDeviceCapabilitiesSpec capabilities;
};

struct TrackDeviceParameterSpec final {
    std::uint32_t index = 0;
    juce::String name;
    float value = 0.0f;
    float defaultValue = 0.0f;
    bool automatable = false;
};

struct TrackDeviceParametersSpec final {
    std::vector<TrackDeviceParameterSpec> parameters;
};

struct TrackDeviceProgramSpec final {
    std::uint32_t index = 0;
    juce::String name;
};

struct TrackDeviceProgramsSpec final {
    std::optional<std::uint32_t> currentIndex;
    std::vector<TrackDeviceProgramSpec> programs;
};

struct TrackPluginStateSpec final {
    PluginStateSpec state;
};

/// Completes `setTrackDeviceProgram` with the state the program loaded.
struct TrackDeviceProgramChangedSpec final {
    PluginStateSpec state;
};

using SidecarResponseSpec =
    std::variant<AudioStatusSpec, TransportAcceptedSpec, TimelineAckSpec, TimelineIdleAckSpec,
                 MidiAckSpec, TrackMixAckSpec, TrackDeviceAckSpec, TrackDeviceStatusSpec,
                 TrackDeviceParametersSpec, TrackDeviceProgramsSpec, TrackPluginStateSpec,
                 TrackDeviceProgramChangedSpec>;

struct SidecarErrorSpec final {
    juce::String kind;
    juce::String message;
    juce::String operation;
    /// Kind-specific diagnostic object, or void when the kind carries none.
    juce::var details;
};

struct ReadySpec final {
    AudioStatusSpec status;
};

struct RecordingCompleteSpec final {
    juce::String directory;
    bool success = false;
    std::optional<juce::String> message;
};

struct TrackPluginStateChangedSpec final {
    juce::String projectId;
    juce::String trackId;
    juce::String deviceId;
    PluginStateSpec state;
};

struct TrackPluginParameterChangedSpec final {
    juce::String projectId;
    juce::String trackId;
    juce::String deviceId;
    std::uint32_t parameterIndex = 0;
    float value = 0.0f;
};

struct FaultSpec final {
    SidecarErrorSpec error;
};

/// Details of a failed audio device switch.
struct DeviceSwitchDetailsSpec final {
    juce::String driver;
    juce::String inputDevice;
    juce::String outputDevice;
    bool restoredPreviousDevice = false;
};

/// Details of a rejected startup input channel.
struct InputChannelDetailsSpec final {
    std::uint32_t inputChannel = 0;
    std::uint32_t availableInputChannels = 0;
};

using SidecarEventSpec =
    std::variant<ReadySpec, AudioStatusSpec, AudioMetersSpec, TransportStatusSpec,
                 RecordingCompleteSpec, TrackPluginStateChangedSpec,
                 TrackPluginParameterChangedSpec, FaultSpec>;

struct OfflineRenderCompleteSpec final {
    std::uint64_t frames = 0;
    std::uint32_t sampleRate = 0;
};

/// Every response `type` written on stdout.
inline constexpr std::array<std::string_view, 12> kSidecarResponseTypes{
    "audioStatus",         "transportAccepted", "timelineAck",
    "timelineIdleAck",     "midiAck",           "trackMixAck",
    "trackDeviceAck",      "trackDeviceStatus", "trackDeviceParameters",
    "trackDevicePrograms", "trackPluginState",  "trackDeviceProgramChanged",
};

/// Every event `type` written on stdout.
inline constexpr std::array<std::string_view, 8> kSidecarEventTypes{
    "ready",
    "audioStatus",
    "audioMeters",
    "transportStatus",
    "recordingComplete",
    "trackPluginStateChanged",
    "trackPluginParameterChanged",
    "fault",
};

/// Returns the `type` written for one response.
[[nodiscard]] std::string_view sidecarResponseType(const SidecarResponseSpec& response) noexcept;
/// Returns the `type` written for one event.
[[nodiscard]] std::string_view sidecarEventType(const SidecarEventSpec& event) noexcept;

[[nodiscard]] juce::var encodeResponse(std::uint64_t requestId,
                                       const SidecarResponseSpec& response);
[[nodiscard]] juce::var encodeError(std::uint64_t requestId, const SidecarErrorSpec& error);
[[nodiscard]] juce::var encodeEvent(const SidecarEventSpec& event);
[[nodiscard]] juce::var encodeDeviceSwitchDetails(const DeviceSwitchDetailsSpec& details);
[[nodiscard]] juce::var encodeInputChannelDetails(const InputChannelDetailsSpec& details);
[[nodiscard]] juce::var encodeOfflineRenderComplete(const OfflineRenderCompleteSpec& result);
[[nodiscard]] juce::var encodeOfflineRenderError(const SidecarErrorSpec& error);

}  // namespace riffra
