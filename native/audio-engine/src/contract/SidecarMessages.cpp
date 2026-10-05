#include "SidecarMessages.h"

#include <type_traits>

namespace riffra {
namespace {

template <class... Visitors>
struct Overloaded : Visitors... {
    using Visitors::operator()...;
};
template <class... Visitors>
Overloaded(Visitors...) -> Overloaded<Visitors...>;

class ObjectBuilder final {
public:
    ObjectBuilder() : object(new juce::DynamicObject()) {}

    ObjectBuilder& set(const char* key, const juce::var& value) {
        object->setProperty(key, value);
        return *this;
    }

    [[nodiscard]] juce::var build() { return juce::var(object.release()); }

private:
    std::unique_ptr<juce::DynamicObject> object;
};

juce::var integer(const std::uint64_t value) { return juce::var(static_cast<juce::int64>(value)); }

juce::var optionalString(const std::optional<juce::String>& value) {
    return value.has_value() ? juce::var(*value) : juce::var();
}

juce::var optionalInteger(const std::optional<std::uint64_t>& value) {
    return value.has_value() ? integer(*value) : juce::var();
}

juce::var optionalNumber(const std::optional<double>& value) {
    return value.has_value() ? juce::var(*value) : juce::var();
}

template <typename Item, typename Encode>
juce::var array(const std::vector<Item>& items, Encode encode) {
    juce::Array<juce::var> values;
    values.ensureStorageAllocated(static_cast<int>(items.size()));
    for (const auto& item : items) values.add(encode(item));
    return values;
}

const char* name(const AudioStateSpec state) noexcept {
    switch (state) {
        case AudioStateSpec::ready:
            return "ready";
        case AudioStateSpec::muted:
            return "muted";
        case AudioStateSpec::faulted:
            return "faulted";
    }
    return "faulted";
}

const char* name(const RecoveryStatusSpec status) noexcept {
    return status == RecoveryStatusSpec::clean ? "clean" : "partial";
}

const char* name(const TransportStateSpec state) noexcept {
    switch (state) {
        case TransportStateSpec::stopped:
            return "stopped";
        case TransportStateSpec::starting:
            return "starting";
        case TransportStateSpec::playing:
            return "playing";
        case TransportStateSpec::faulted:
            return "faulted";
    }
    return "faulted";
}

const char* name(const RecordingPhaseSpec phase) noexcept {
    switch (phase) {
        case RecordingPhaseSpec::idle:
            return "idle";
        case RecordingPhaseSpec::countingIn:
            return "countingIn";
        case RecordingPhaseSpec::recording:
            return "recording";
        case RecordingPhaseSpec::stopping:
            return "stopping";
    }
    return "idle";
}

juce::var encodeChannel(const AudioChannelSpec& channel) {
    return ObjectBuilder{}.set("index", integer(channel.index)).set("name", channel.name).build();
}

juce::var encodeMidiDevice(const MidiDeviceSpec& device) {
    return ObjectBuilder{}.set("id", device.id).set("name", device.name).build();
}

juce::var encodeRecording(const RecordingStatusSpec& recording) {
    return ObjectBuilder{}
        .set("active", recording.active)
        .set("processing", recording.processing)
        .set("cancelled", recording.cancelled)
        .set("directory", optionalString(recording.directory))
        .set("sampleRate", optionalNumber(recording.sampleRate))
        .set("samplesWritten", integer(recording.samplesWritten))
        .set("droppedMidiEvents", integer(recording.droppedMidiEvents))
        .set("droppedBlocks", integer(recording.droppedBlocks))
        .set("missingSamples", integer(recording.missingSamples))
        .set("rawAttemptedSamples", integer(recording.rawAttemptedSamples))
        .set("processedAttemptedSamples", integer(recording.processedAttemptedSamples))
        .set("rawDroppedBlocks", integer(recording.rawDroppedBlocks))
        .set("processedDroppedBlocks", integer(recording.processedDroppedBlocks))
        .set("rawMissingSamples", integer(recording.rawMissingSamples))
        .set("processedMissingSamples", integer(recording.processedMissingSamples))
        .set("rawDropoutStartSample", optionalInteger(recording.rawDropoutStartSample))
        .set("rawDropoutEndSample", optionalInteger(recording.rawDropoutEndSample))
        .set("processedDropoutStartSample", optionalInteger(recording.processedDropoutStartSample))
        .set("processedDropoutEndSample", optionalInteger(recording.processedDropoutEndSample))
        .set("recoveryStatus", name(recording.recoveryStatus))
        .set("error", optionalString(recording.error))
        .build();
}

juce::var encodeInstrumentFault(const InstrumentFaultSpec& fault) {
    return ObjectBuilder{}
        .set("trackId", fault.trackId)
        .set("instrumentType", fault.instrumentType)
        .set("faultCode", integer(fault.faultCode))
        .set("droppedMidiEvents", integer(fault.droppedMidiEvents))
        .build();
}

juce::var encodeWindow(const CallbackWindowSpec& window) {
    return ObjectBuilder{}
        .set("durationMs", integer(window.durationMs))
        .set("callbackCount", integer(window.callbackCount))
        .set("overruns", integer(window.overruns))
        .set("averageCallbackDurationUs", integer(window.averageCallbackDurationUs))
        .set("maximumCallbackDurationUs", integer(window.maximumCallbackDurationUs))
        .build();
}

juce::var encodeRealtime(const RealtimeDiagnosticsSpec& realtime) {
    return ObjectBuilder{}
        .set("callbackCount", integer(realtime.callbackCount))
        .set("callbackOverruns", integer(realtime.callbackOverruns))
        .set("window", encodeWindow(realtime.window))
        .build();
}

juce::var encodeTrackLoad(const TrackLoadSpec& load) {
    return ObjectBuilder{}
        .set("trackId", load.trackId)
        .set("averageProcessingUs", integer(load.averageProcessingUs))
        .set("maximumProcessingUs", integer(load.maximumProcessingUs))
        .build();
}

juce::var encodeDiagnostics(const AudioDiagnosticsSpec& diagnostics) {
    return ObjectBuilder{}
        .set("realtime", encodeRealtime(diagnostics.realtime))
        .set("trackLoads", array(diagnostics.trackLoads, encodeTrackLoad))
        .set("preLimiterPeak", diagnostics.preLimiterPeak)
        .set("limiterGainReductionDb", diagnostics.limiterGainReductionDb)
        .set("hardClipSamples", integer(diagnostics.hardClipSamples))
        .set("liveMidiDrops", integer(diagnostics.liveMidiDrops))
        .set("graphRevision", integer(diagnostics.graphRevision))
        .set("graphPublishCount", integer(diagnostics.graphPublishCount))
        .set("trackCount", integer(diagnostics.trackCount))
        .set("instrumentRuntimeCount", integer(diagnostics.instrumentRuntimeCount))
        .set("pluginCount", integer(diagnostics.pluginCount))
        .set("maximumLatencySamples", integer(diagnostics.maximumLatencySamples))
        .set("instrumentFaults", array(diagnostics.instrumentFaults, encodeInstrumentFault))
        .build();
}

ObjectBuilder audioStatusFields(const AudioStatusSpec& status) {
    ObjectBuilder builder;
    builder.set("state", name(status.state))
        .set("message", status.message)
        .set("driver", optionalString(status.driver))
        .set("inputDevice", optionalString(status.inputDevice))
        .set("inputChannel",
             status.inputChannel.has_value() ? integer(*status.inputChannel) : juce::var())
        .set("inputChannels", array(status.inputChannels, encodeChannel))
        .set("activeInputChannels",
             array(status.activeInputChannels, [](const auto index) { return integer(index); }))
        .set("outputDevice", optionalString(status.outputDevice))
        .set("outputChannels", array(status.outputChannels, encodeChannel))
        .set("activeOutputChannels",
             array(status.activeOutputChannels, [](const auto index) { return integer(index); }))
        .set("sampleRate", optionalNumber(status.sampleRate))
        .set("bufferSize",
             status.bufferSize.has_value() ? integer(*status.bufferSize) : juce::var())
        .set("roundTripMs", optionalNumber(status.roundTripMs))
        .set("timelineTick", optionalInteger(status.timelineTick))
        .set("recording", encodeRecording(status.recording))
        .set("midiInputs", array(status.midiInputs, encodeMidiDevice))
        .set("midiOutputs", array(status.midiOutputs, encodeMidiDevice))
        .set("midiInputActive", status.midiInputActive)
        .set("midiMessages", integer(status.midiMessages))
        .set("lastMidiNote",
             status.lastMidiNote.has_value() ? integer(*status.lastMidiNote) : juce::var())
        .set("inputPeak", status.inputPeak)
        .set("outputPeak", status.outputPeak)
        .set("invalidSamples", integer(status.invalidSamples))
        .set("feedbackSuspected", status.feedbackSuspected)
        .set("previewing", status.previewing)
        .set("instrumentPreviewing", status.instrumentPreviewing)
        .set("muteReasons", integer(status.muteReasons))
        .set("diagnostics", encodeDiagnostics(status.diagnostics));
    return builder;
}

juce::var encodeAudioStatus(const AudioStatusSpec& status) {
    return audioStatusFields(status).build();
}

juce::var encodeTrackMeter(const TrackMeterSpec& meter) {
    return ObjectBuilder{}
        .set("trackId", meter.trackId)
        .set("peakLeft", meter.peakLeft)
        .set("peakRight", meter.peakRight)
        .set("rmsLeft", meter.rmsLeft)
        .set("rmsRight", meter.rmsRight)
        .build();
}

ObjectBuilder audioMetersFields(const AudioMetersSpec& meters) {
    ObjectBuilder builder;
    builder.set("projectId", optionalString(meters.projectId))
        .set("inputPeak", meters.inputPeak)
        .set("outputPeak", meters.outputPeak)
        .set("outputPeakLeft", meters.outputPeakLeft)
        .set("outputPeakRight", meters.outputPeakRight)
        .set("invalidSamples", integer(meters.invalidSamples))
        .set("preLimiterPeak", meters.preLimiterPeak)
        .set("limiterGainReductionDb", meters.limiterGainReductionDb)
        .set("hardClipSamples", integer(meters.hardClipSamples))
        .set("muteReasons", integer(meters.muteReasons))
        .set("feedbackSuspected", meters.feedbackSuspected)
        .set("previewing", meters.previewing)
        .set("instrumentPreviewing", meters.instrumentPreviewing)
        .set("trackMeters", array(meters.trackMeters, encodeTrackMeter));
    return builder;
}

ObjectBuilder transportStatusFields(const TransportStatusSpec& status) {
    ObjectBuilder builder;
    builder.set("state", name(status.state))
        .set("revision", optionalInteger(status.revision))
        .set("timelineTick", integer(status.timelineTick))
        .set("timelineSample", juce::var(static_cast<juce::int64>(status.timelineSample)))
        .set("audioClockSample", integer(status.audioClockSample))
        .set("sampleRate", optionalNumber(status.sampleRate))
        .set("appliedCommandSequence", integer(status.appliedCommandSequence))
        .set("recordingPhase", name(status.recordingPhase))
        .set("recordingStartTick", integer(status.recordingStartTick))
        .set("recordingPassOrdinal", integer(status.recordingPassOrdinal))
        .set("armedTrackIds",
             array(status.armedTrackIds, [](const juce::String& id) { return juce::var(id); }))
        .set("instrumentFaults", array(status.instrumentFaults, encodeInstrumentFault))
        .set("clockGeneration", integer(status.clockGeneration))
        .set("discontinuity", integer(status.discontinuity));
    return builder;
}

juce::var encodePluginState(const PluginStateSpec& state) {
    return ObjectBuilder{}
        .set("parameterValues",
             array(state.parameterValues, [](const float value) { return juce::var(value); }))
        .set("stateData", optionalString(state.stateData))
        .set("bypassed", state.bypassed)
        .build();
}

ObjectBuilder errorFields(const SidecarErrorSpec& error) {
    ObjectBuilder builder;
    builder.set("kind", error.kind)
        .set("message", error.message)
        .set("operation", error.operation)
        .set("details", error.details);
    return builder;
}

juce::var typeOnly(const char* type) { return ObjectBuilder{}.set("type", type).build(); }

juce::var encodeResponseBody(const SidecarResponseSpec& response) {
    return std::visit(
        Overloaded{
            [](const AudioStatusSpec& status) {
                return audioStatusFields(status).set("type", "audioStatus").build();
            },
            [](const TransportAcceptedSpec& accepted) {
                return ObjectBuilder{}
                    .set("type", "transportAccepted")
                    .set("commandSequence", integer(accepted.commandSequence))
                    .build();
            },
            [](const TimelineAckSpec&) { return typeOnly("timelineAck"); },
            [](const TimelineIdleAckSpec&) { return typeOnly("timelineIdleAck"); },
            [](const MidiAckSpec&) { return typeOnly("midiAck"); },
            [](const TrackMixAckSpec&) { return typeOnly("trackMixAck"); },
            [](const TrackDeviceAckSpec&) { return typeOnly("trackDeviceAck"); },
            [](const TrackDeviceStatusSpec& status) {
                return ObjectBuilder{}
                    .set("type", "trackDeviceStatus")
                    .set("name", status.name)
                    .set("bypassed", status.bypassed)
                    .set("parameterCount", integer(status.parameterCount))
                    .set("capabilities", ObjectBuilder{}
                                             .set("parameters", status.capabilities.parameters)
                                             .set("state", status.capabilities.state)
                                             .set("presets", status.capabilities.presets)
                                             .set("editor", status.capabilities.editor)
                                             .build())
                    .build();
            },
            [](const TrackDeviceParametersSpec& status) {
                return ObjectBuilder{}
                    .set("type", "trackDeviceParameters")
                    .set("parameters",
                         array(status.parameters,
                               [](const TrackDeviceParameterSpec& parameter) {
                                   return ObjectBuilder{}
                                       .set("index", integer(parameter.index))
                                       .set("name", parameter.name)
                                       .set("value", parameter.value)
                                       .set("defaultValue", parameter.defaultValue)
                                       .set("automatable", parameter.automatable)
                                       .set("displayValue", parameter.displayValue)
                                       .set("label", parameter.label)
                                       .set("discrete", parameter.discrete)
                                       .set("stepCount", integer(parameter.stepCount))
                                       .set("choices",
                                            array(parameter.choices,
                                                  [](const TrackDeviceParameterChoiceSpec& choice) {
                                                      return ObjectBuilder{}
                                                          .set("value", choice.value)
                                                          .set("displayValue", choice.displayValue)
                                                          .build();
                                                  }))
                                       .build();
                               }))
                    .build();
            },
            [](const TrackDeviceProgramsSpec& status) {
                return ObjectBuilder{}
                    .set("type", "trackDevicePrograms")
                    .set("currentIndex", status.currentIndex.has_value()
                                             ? integer(*status.currentIndex)
                                             : juce::var())
                    .set("programs", array(status.programs,
                                           [](const TrackDeviceProgramSpec& program) {
                                               return ObjectBuilder{}
                                                   .set("index", integer(program.index))
                                                   .set("name", program.name)
                                                   .build();
                                           }))
                    .build();
            },
            [](const TrackPluginStateSpec& status) {
                return ObjectBuilder{}
                    .set("type", "trackPluginState")
                    .set("state", encodePluginState(status.state))
                    .build();
            },
            [](const TrackDeviceProgramChangedSpec& changed) {
                return ObjectBuilder{}
                    .set("type", "trackDeviceProgramChanged")
                    .set("state", encodePluginState(changed.state))
                    .build();
            },
        },
        response);
}

juce::var encodeEventBody(const SidecarEventSpec& event) {
    return std::visit(
        Overloaded{
            [](const ReadySpec& ready) {
                return ObjectBuilder{}
                    .set("type", "ready")
                    .set("protocolVersion", integer(kSidecarProtocolVersion))
                    .set("status", encodeAudioStatus(ready.status))
                    .build();
            },
            [](const AudioStatusSpec& status) {
                return audioStatusFields(status).set("type", "audioStatus").build();
            },
            [](const AudioMetersSpec& meters) {
                return audioMetersFields(meters).set("type", "audioMeters").build();
            },
            [](const TransportStatusSpec& status) {
                return transportStatusFields(status).set("type", "transportStatus").build();
            },
            [](const RecordingCompleteSpec& completion) {
                return ObjectBuilder{}
                    .set("type", "recordingComplete")
                    .set("directory", completion.directory)
                    .set("success", completion.success)
                    .set("message", optionalString(completion.message))
                    .build();
            },
            [](const TrackPluginStateChangedSpec& changed) {
                return ObjectBuilder{}
                    .set("type", "trackPluginStateChanged")
                    .set("projectId", changed.projectId)
                    .set("trackId", changed.trackId)
                    .set("deviceId", changed.deviceId)
                    .set("state", encodePluginState(changed.state))
                    .build();
            },
            [](const TrackPluginParameterChangedSpec& changed) {
                return ObjectBuilder{}
                    .set("type", "trackPluginParameterChanged")
                    .set("projectId", changed.projectId)
                    .set("trackId", changed.trackId)
                    .set("deviceId", changed.deviceId)
                    .set("parameterIndex", integer(changed.parameterIndex))
                    .set("value", changed.value)
                    .build();
            },
            [](const FaultSpec& fault) {
                return errorFields(fault.error).set("type", "fault").build();
            },
        },
        event);
}

}  // namespace

juce::var encodeAudioDeviceProbe(const AudioDeviceProbeSpec& probe) {
    const auto encodeDevice = [](const AudioDeviceProbeSpec::Device& device) {
        return ObjectBuilder{}
            .set("name", device.name)
            .set("channels", array(device.channels, encodeChannel))
            .build();
    };
    const auto encodeDriver = [&](const AudioDeviceProbeSpec::Driver& driver) {
        return ObjectBuilder{}
            .set("name", driver.name)
            .set("accessMode", driver.accessMode)
            .set("devicePairing", driver.devicePairing)
            .set("inputs", array(driver.inputs, encodeDevice))
            .set("outputs", array(driver.outputs, encodeDevice))
            .build();
    };
    return ObjectBuilder{}
        .set("type", "audioDeviceProbe")
        .set("drivers", array(probe.drivers, encodeDriver))
        .set("refreshedAtMs", integer(probe.refreshedAtMs))
        .set("message", probe.message)
        .set("muteReasons", integer(probe.muteReasons))
        .set("limiterCeiling", probe.limiterCeiling)
        .build();
}

juce::var encodeDeviceChannels(const DeviceChannelsSpec& channels) {
    return ObjectBuilder{}
        .set("type", "deviceChannels")
        .set("driver", channels.driver)
        .set("inputDevice", channels.inputDevice)
        .set("inputChannels", array(channels.inputChannels, encodeChannel))
        .set("outputDevice", channels.outputDevice)
        .set("outputChannels", array(channels.outputChannels, encodeChannel))
        .build();
}

juce::var encodePluginScanResult(const PluginScanResultSpec& result) {
    const auto encodePlugin = [](const juce::PluginDescription& plugin) {
        return ObjectBuilder{}
            .set("name", plugin.name)
            .set("descriptiveName", plugin.descriptiveName)
            .set("vendor", plugin.manufacturerName)
            .set("version", plugin.version)
            .set("category", plugin.category)
            .set("format", plugin.pluginFormatName)
            .set("path", plugin.fileOrIdentifier)
            .set("identifier", plugin.createIdentifierString())
            .set("uniqueId", juce::var(static_cast<juce::int64>(plugin.uniqueId)))
            .set("deprecatedUid", juce::var(static_cast<juce::int64>(plugin.deprecatedUid)))
            .set("numInputs", plugin.numInputChannels)
            .set("numOutputs", plugin.numOutputChannels)
            .set("isInstrument", plugin.isInstrument)
            .set("hasSharedContainer", plugin.hasSharedContainer)
            .set("lastFileModifiedMs", juce::var(plugin.lastFileModTime.toMilliseconds()))
            .set("lastInfoUpdatedMs", juce::var(plugin.lastInfoUpdateTime.toMilliseconds()))
            .build();
    };
    return ObjectBuilder{}
        .set("type", "pluginScanResult")
        .set("path", result.path)
        .set("plugins", array(result.plugins, encodePlugin))
        .set("loadTested", result.loadTested)
        .set("loadTestMessage", result.loadTestMessage)
        .set("loadTestDurationMs", result.loadTestDurationMs)
        .set("scanDurationMs", result.scanDurationMs)
        .build();
}

juce::var encodePluginScanError(const PluginScanErrorSpec& error) {
    return ObjectBuilder{}
        .set("type", "pluginScanError")
        .set("kind", "pluginScanRejected")
        .set("path", error.path)
        .set("message", error.message)
        .set("operation", "plugin.scan")
        .set("details", ObjectBuilder{}
                            .set("path", error.path.isEmpty() ? juce::var() : juce::var(error.path))
                            .build())
        .build();
}

juce::var encodePluginLoadTestResult(const PluginLoadTestResultSpec& result) {
    return ObjectBuilder{}
        .set("type", "pluginLoadTestResult")
        .set("path", result.path)
        .set("success", result.success)
        .set("message", result.message)
        .set("durationMs", result.durationMs)
        .build();
}

std::string_view sidecarResponseType(const SidecarResponseSpec& response) noexcept {
    return kSidecarResponseTypes[response.index()];
}

std::string_view sidecarEventType(const SidecarEventSpec& event) noexcept {
    return kSidecarEventTypes[event.index()];
}

juce::var encodeResponse(const std::uint64_t requestId, const SidecarResponseSpec& response) {
    return ObjectBuilder{}
        .set("kind", "response")
        .set("requestId", integer(requestId))
        .set("response", encodeResponseBody(response))
        .build();
}

juce::var encodeError(const std::uint64_t requestId, const SidecarErrorSpec& error) {
    return ObjectBuilder{}
        .set("kind", "error")
        .set("requestId", integer(requestId))
        .set("error", errorFields(error).build())
        .build();
}

juce::var encodeEvent(const SidecarEventSpec& event) {
    return ObjectBuilder{}.set("kind", "event").set("event", encodeEventBody(event)).build();
}

juce::var encodeDeviceSwitchDetails(const DeviceSwitchDetailsSpec& details) {
    return ObjectBuilder{}
        .set("driver", details.driver)
        .set("inputDevice", details.inputDevice)
        .set("outputDevice", details.outputDevice)
        .set("restoredPreviousDevice", details.restoredPreviousDevice)
        .build();
}

juce::var encodeInputChannelDetails(const InputChannelDetailsSpec& details) {
    return ObjectBuilder{}
        .set("inputChannel", integer(details.inputChannel))
        .set("availableInputChannels", integer(details.availableInputChannels))
        .build();
}

juce::var encodeOfflineRenderComplete(const OfflineRenderCompleteSpec& result) {
    return ObjectBuilder{}
        .set("type", "offlineRenderComplete")
        .set("frames", integer(result.frames))
        .set("sampleRate", integer(result.sampleRate))
        .build();
}

juce::var encodeOfflineRenderError(const SidecarErrorSpec& error) {
    return errorFields(error).set("type", "error").build();
}

}  // namespace riffra
