#include "SidecarCommands.h"

#include <cmath>
#include <initializer_list>
#include <limits>
#include <utility>

#include "ContractReader.h"
#include "ExecutionGraphDecoder.h"
#include "audio/InstrumentPreviewContract.h"

namespace riffra {
namespace {

constexpr auto kCommandPath = "command";

bool fail(juce::String& error, const juce::String& path, const juce::String& message) {
    if (error.isEmpty()) error = path + ": " + message;
    return false;
}

juce::String field(const char* key) { return juce::String(kCommandPath) + "." + key; }

/// Decodes a command whose only key is `type`.
template <typename Command>
bool decodeEmpty(const juce::var& value, SidecarCommandSpec& output, juce::String& error) {
    ContractReader reader(value, kCommandPath, {"type"}, error);
    juce::String type;
    if (!reader.string("type", type) || !reader.finish()) return false;
    output = Command{};
    return true;
}

bool decodeTrackDevice(ContractReader& reader, juce::String& trackId, juce::String& deviceId) {
    return reader.string("trackId", trackId) && reader.string("deviceId", deviceId);
}

bool decodeMidiMessage(const juce::Array<juce::var>& bytes, juce::MidiMessage& message,
                       juce::String& error) {
    if (bytes.isEmpty() || bytes.size() > 3)
        return fail(error, field("bytes"), "must contain between 1 and 3 bytes");
    std::array<std::uint8_t, 3> values{};
    for (int index = 0; index < bytes.size(); ++index) {
        const auto& byte = bytes.getReference(index);
        if (!byte.isInt() && !byte.isInt64())
            return fail(error, field("bytes"), "must contain integers");
        const auto numeric = static_cast<juce::int64>(byte);
        const auto limit = index == 0 ? 0xff : 0x7f;
        if (numeric < (index == 0 ? 0x80 : 0) || numeric > limit)
            return fail(
                error, field("bytes"),
                index == 0 ? "must start with a status byte" : "data bytes must be below 128");
        values[static_cast<std::size_t>(index)] = static_cast<std::uint8_t>(numeric);
    }
    switch (bytes.size()) {
        case 1:
            message = juce::MidiMessage(values[0]);
            break;
        case 2:
            message = juce::MidiMessage(values[0], values[1]);
            break;
        default:
            message = juce::MidiMessage(values[0], values[1], values[2]);
            break;
    }
    return true;
}

bool decodePreviewNote(const juce::var& value, const juce::String& path,
                       InstrumentPreviewNote& output, juce::String& error) {
    ContractReader reader(value, path, {"tick", "durationTicks", "note", "velocity"}, error);
    std::uint8_t note = 0;
    std::uint8_t velocity = 0;
    if (!reader.unsignedInteger("tick", output.tick) ||
        !reader.unsignedInteger("durationTicks", output.durationTicks) ||
        !reader.unsigned8("note", note) || !reader.unsigned8("velocity", velocity) ||
        !reader.finish())
        return false;
    if (note > instrument_preview::kMaximumMidiValue)
        return fail(error, path + ".note", "must be at most 127");
    if (velocity == 0 || velocity > instrument_preview::kMaximumMidiValue)
        return fail(error, path + ".velocity", "must be between 1 and 127");
    if (output.durationTicks == 0) return fail(error, path + ".durationTicks", "must be positive");
    output.note = note;
    output.velocity = velocity;
    return true;
}

bool decodePreview(const juce::var& value, InstrumentPreviewSpec& output, juce::String& error) {
    const auto path = field("preview");
    ContractReader reader(
        value, path, {"tempoBpm", "ticksPerBeat", "timeSignature", "lengthTicks", "notes"}, error);
    std::uint32_t ticksPerBeat = 0;
    juce::var timeSignature;
    juce::Array<juce::var> notes;
    if (!reader.number("tempoBpm", output.tempoBpm) ||
        !reader.unsigned32("ticksPerBeat", ticksPerBeat) ||
        !reader.object("timeSignature", timeSignature) ||
        !reader.unsignedInteger("lengthTicks", output.lengthTicks) ||
        !reader.array("notes", notes) || !reader.finish())
        return false;
    if (ticksPerBeat > instrument_preview::kMaximumTicksPerBeat ||
        !instrument_preview::isValidTicksPerBeat(static_cast<std::uint16_t>(ticksPerBeat)))
        return fail(error, path + ".ticksPerBeat", "is outside the supported range");
    output.ticksPerBeat = static_cast<std::uint16_t>(ticksPerBeat);
    if (!instrument_preview::isWithinDurationLimit(output.tempoBpm, output.ticksPerBeat,
                                                   output.lengthTicks))
        return fail(error, path, "has an invalid tempo or length");

    ContractReader signature(timeSignature, path + ".timeSignature", {"numerator", "denominator"},
                             error);
    if (!signature.unsigned8("numerator", output.timeSignature.numerator) ||
        !signature.unsigned8("denominator", output.timeSignature.denominator) ||
        !signature.finish())
        return false;
    if (!instrument_preview::isValidNumerator(output.timeSignature.numerator) ||
        !instrument_preview::isValidDenominator(output.timeSignature.denominator))
        return fail(error, path + ".timeSignature", "is invalid");

    if (notes.size() < static_cast<int>(instrument_preview::kMinimumNoteCount) ||
        notes.size() > static_cast<int>(instrument_preview::kMaximumNoteCount))
        return fail(error, path + ".notes", "has an unsupported note count");
    output.notes.clear();
    output.notes.reserve(static_cast<std::size_t>(notes.size()));
    for (int index = 0; index < notes.size(); ++index) {
        const auto notePath = path + ".notes[" + juce::String(index) + "]";
        InstrumentPreviewNote note;
        if (!decodePreviewNote(notes.getReference(index), notePath, note, error)) return false;
        if (note.tick >= output.lengthTicks || note.durationTicks > output.lengthTicks ||
            note.tick > output.lengthTicks - note.durationTicks)
            return fail(error, notePath, "extends beyond the preview length");
        if (!output.notes.empty() && note.tick < output.notes.back().tick)
            return fail(error, notePath, "is out of order");
        output.notes.push_back(note);
    }
    return true;
}

bool decodeCommand(const juce::var& value, SidecarCommandSpec& output, juce::String& error) {
    if (!value.isObject()) return fail(error, kCommandPath, "expected object");
    const auto typeValue = value.getProperty("type", {});
    if (!typeValue.isString()) return fail(error, field("type"), "expected string");
    const auto type = typeValue.toString();

    const auto reader = [&](std::initializer_list<const char*> keys) {
        return ContractReader(value, kCommandPath, keys, error);
    };
    juce::String ignoredType;

    if (type == "status") return decodeEmpty<StatusCommand>(value, output, error);
    if (type == "setEmergencyMute") {
        auto fields = reader({"type", "muted"});
        SetEmergencyMuteCommand command;
        if (!fields.string("type", ignoredType) || !fields.boolean("muted", command.muted) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "setFeedbackProtection" || type == "setEngineTransitionMute") {
        auto fields = reader({"type", "active"});
        bool active = false;
        if (!fields.string("type", ignoredType) || !fields.boolean("active", active) ||
            !fields.finish())
            return false;
        if (type == "setFeedbackProtection")
            output = SetFeedbackProtectionCommand{active};
        else
            output = SetEngineTransitionMuteCommand{active};
        return true;
    }
    if (type == "previewMasterGainDb") {
        auto fields = reader({"type", "gainDb"});
        PreviewMasterGainDbCommand command;
        if (!fields.string("type", ignoredType) || !fields.number("gainDb", command.gainDb) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "prepareTimelineSnapshot") {
        auto fields = reader({"type", "snapshot"});
        juce::var snapshot;
        if (!fields.string("type", ignoredType) || !fields.object("snapshot", snapshot) ||
            !fields.finish())
            return false;
        PrepareTimelineSnapshotCommand command;
        juce::String snapshotError;
        if (!decodeTimelineSnapshot(snapshot, command.snapshot, snapshotError))
            return fail(error, field("snapshot"), snapshotError);
        output = std::move(command);
        return true;
    }
    if (type == "commitTimelineSnapshot")
        return decodeEmpty<CommitTimelineSnapshotCommand>(value, output, error);
    if (type == "discardTimelineSnapshot")
        return decodeEmpty<DiscardTimelineSnapshotCommand>(value, output, error);
    if (type == "waitForTimelineIdle") {
        auto fields = reader({"type", "timeoutMs"});
        WaitForTimelineIdleCommand command;
        if (!fields.string("type", ignoredType) ||
            !fields.unsignedInteger("timeoutMs", command.timeoutMs) || !fields.finish())
            return false;
        if (command.timeoutMs == 0) return fail(error, field("timeoutMs"), "must be positive");
        output = command;
        return true;
    }
    if (type == "playTimeline" || type == "setTransportStarting" || type == "stopTimeline") {
        auto fields = reader({"type"});
        if (!fields.string("type", ignoredType) || !fields.finish()) return false;
        output =
            TransportCommand{type == "playTimeline"           ? TransportCommandKind::play
                             : type == "setTransportStarting" ? TransportCommandKind::setStarting
                                                              : TransportCommandKind::stop,
                             0};
        return true;
    }
    if (type == "seekTimeline") {
        auto fields = reader({"type", "tick"});
        TransportCommand command{TransportCommandKind::seek, 0};
        if (!fields.string("type", ignoredType) || !fields.unsignedInteger("tick", command.tick) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "enableMidiListening" || type == "disableMidiListening") {
        auto fields = reader({"type"});
        if (!fields.string("type", ignoredType) || !fields.finish()) return false;
        output = SetMidiListeningCommand{type == "enableMidiListening"};
        return true;
    }
    if (type == "setLiveMidiTarget") {
        auto fields = reader({"type", "trackId"});
        SetLiveMidiTargetCommand command;
        if (!fields.string("type", ignoredType) ||
            !fields.optionalString("trackId", command.trackId) || !fields.finish())
            return false;
        if (command.trackId.has_value() && command.trackId->isEmpty())
            return fail(error, field("trackId"), "must not be empty");
        output = command;
        return true;
    }
    if (type == "sendTrackMidi") {
        auto fields = reader({"type", "trackId", "bytes"});
        SendTrackMidiCommand command;
        juce::Array<juce::var> bytes;
        if (!fields.string("type", ignoredType) || !fields.string("trackId", command.trackId) ||
            !fields.array("bytes", bytes) || !fields.finish() ||
            !decodeMidiMessage(bytes, command.message, error))
            return false;
        output = command;
        return true;
    }
    if (type == "panicTrackMidi") {
        auto fields = reader({"type", "trackId"});
        PanicTrackMidiCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("trackId", command.trackId) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "setTrackMix") {
        auto fields = reader({"type", "trackId", "gainDb", "pan"});
        SetTrackMixCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("trackId", command.trackId) ||
            !fields.optionalNumber("gainDb", command.gainDb) ||
            !fields.optionalNumber("pan", command.pan) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "setTrackDeviceBypassed") {
        auto fields = reader({"type", "trackId", "deviceId", "bypassed"});
        SetTrackDeviceBypassedCommand command;
        if (!fields.string("type", ignoredType) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) ||
            !fields.boolean("bypassed", command.bypassed) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "setTrackDeviceParameter") {
        auto fields = reader({"type", "trackId", "deviceId", "parameterIndex", "value"});
        SetTrackDeviceParameterCommand command;
        double parameterValue = 0.0;
        if (!fields.string("type", ignoredType) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) ||
            !fields.unsigned32("parameterIndex", command.parameterIndex) ||
            !fields.number("value", parameterValue) || !fields.finish())
            return false;
        if (parameterValue < 0.0 || parameterValue > 1.0)
            return fail(error, field("value"), "must be between 0 and 1");
        command.value = static_cast<float>(parameterValue);
        output = command;
        return true;
    }
    if (type == "getTrackDeviceStatus" || type == "getTrackDeviceParameters" ||
        type == "getTrackDevicePrograms" || type == "getTrackPluginState") {
        auto fields = reader({"type", "trackId", "deviceId"});
        GetTrackDeviceCommand command;
        command.query = type == "getTrackDeviceStatus"       ? TrackDeviceQuery::status
                        : type == "getTrackDeviceParameters" ? TrackDeviceQuery::parameters
                        : type == "getTrackDevicePrograms"   ? TrackDeviceQuery::programs
                                                             : TrackDeviceQuery::pluginState;
        if (!fields.string("type", ignoredType) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "setTrackPluginState") {
        auto fields = reader({"type", "trackId", "deviceId", "state"});
        SetTrackPluginStateCommand command;
        juce::var state;
        if (!fields.string("type", ignoredType) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) ||
            !fields.object("state", state) || !fields.finish())
            return false;
        juce::String stateError;
        if (!decodePluginState(state, command.state, stateError))
            return fail(error, field("state"), stateError);
        output = std::move(command);
        return true;
    }
    if (type == "setTrackDeviceProgram") {
        auto fields = reader({"type", "trackId", "deviceId", "programIndex"});
        SetTrackDeviceProgramCommand command;
        if (!fields.string("type", ignoredType) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) ||
            !fields.unsigned32("programIndex", command.programIndex) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "openTrackPluginEditor") {
        auto fields = reader({"type", "projectId", "trackId", "deviceId"});
        OpenTrackPluginEditorCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("projectId", command.projectId) ||
            !decodeTrackDevice(fields, command.trackId, command.deviceId) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "previewSample") {
        auto fields = reader({"type", "path", "startMs", "endMs", "gain", "loop"});
        PreviewSampleCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("path", command.path) ||
            !fields.unsignedInteger("startMs", command.startMs) ||
            !fields.optionalUnsignedInteger("endMs", command.endMs) ||
            !fields.number("gain", command.gain) || !fields.boolean("loop", command.loop) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "previewInstrument") {
        auto fields = reader({"type", "definitionJson", "definitionBaseDir", "preview"});
        PreviewInstrumentCommand command;
        juce::var preview;
        if (!fields.string("type", ignoredType) ||
            !fields.string("definitionJson", command.definitionJson) ||
            !fields.string("definitionBaseDir", command.definitionBaseDir) ||
            !fields.object("preview", preview) || !fields.finish() ||
            !decodePreview(preview, command.preview, error))
            return false;
        output = std::move(command);
        return true;
    }
    if (type == "stopPreview") return decodeEmpty<StopPreviewCommand>(value, output, error);
    if (type == "stopInstrumentPreview")
        return decodeEmpty<StopInstrumentPreviewCommand>(value, output, error);
    if (type == "startTakeComparison") {
        auto fields = reader({"type", "rawPath", "processedPath", "rawStartFrame", "rawEndFrame",
                              "processedStartFrame", "processedEndFrame"});
        StartTakeComparisonCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("rawPath", command.rawPath) ||
            !fields.string("processedPath", command.processedPath) ||
            !fields.unsignedInteger("rawStartFrame", command.rawStartFrame) ||
            !fields.unsignedInteger("rawEndFrame", command.rawEndFrame) ||
            !fields.unsignedInteger("processedStartFrame", command.processedStartFrame) ||
            !fields.unsignedInteger("processedEndFrame", command.processedEndFrame) ||
            !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "switchTakeComparisonVariant") {
        auto fields = reader({"type", "variant"});
        juce::String variant;
        if (!fields.string("type", ignoredType) || !fields.string("variant", variant) ||
            !fields.finish())
            return false;
        if (variant != "raw" && variant != "processed")
            return fail(error, field("variant"), "unknown value \"" + variant + "\"");
        output = SwitchTakeComparisonVariantCommand{variant == "raw"
                                                        ? TakeComparisonVariantSpec::raw
                                                        : TakeComparisonVariantSpec::processed};
        return true;
    }
    if (type == "stopTakeComparison")
        return decodeEmpty<StopTakeComparisonCommand>(value, output, error);
    if (type == "recoverAudioDevice")
        return decodeEmpty<RecoverAudioDeviceCommand>(value, output, error);
    if (type == "setAudioDriver") {
        auto fields = reader({"type", "driver", "inputDevice", "inputChannel", "outputDevice",
                              "sampleRate", "bufferSize"});
        SetAudioDriverCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("driver", command.driver) ||
            !fields.optionalString("inputDevice", command.inputDevice) ||
            !fields.unsigned32("inputChannel", command.inputChannel) ||
            !fields.optionalString("outputDevice", command.outputDevice) ||
            !fields.optionalUnsigned32("sampleRate", command.sampleRate) ||
            !fields.optionalUnsigned32("bufferSize", command.bufferSize) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "startArrangeRecording") {
        auto fields = reader({"type", "directory", "countInBeats"});
        StartArrangeRecordingCommand command;
        if (!fields.string("type", ignoredType) || !fields.string("directory", command.directory) ||
            !fields.unsigned8("countInBeats", command.countInBeats) || !fields.finish())
            return false;
        output = command;
        return true;
    }
    if (type == "stopArrangeRecording")
        return decodeEmpty<StopArrangeRecordingCommand>(value, output, error);
    return fail(error, field("type"), "unknown command \"" + type + "\"");
}

}  // namespace

std::optional<std::uint64_t> readSidecarRequestId(const juce::var& envelope) {
    const auto requestId = envelope.getProperty("requestId", {});
    if (!requestId.isInt() && !requestId.isInt64()) return std::nullopt;
    const auto value = static_cast<juce::int64>(requestId);
    if (value <= 0) return std::nullopt;
    return static_cast<std::uint64_t>(value);
}

bool decodeSidecarRequest(const juce::var& envelope, SidecarRequestSpec& output,
                          juce::String& error) {
    if (!envelope.isObject()) return fail(error, "request", "expected one JSON object per line");
    ContractReader reader(envelope, "", {"requestId", "command"}, error);
    juce::var command;
    if (!reader.unsignedInteger("requestId", output.requestId) ||
        !reader.object("command", command) || !reader.finish())
        return false;
    if (output.requestId == 0) return fail(error, "requestId", "must be positive");
    return decodeCommand(command, output.command, error);
}

}  // namespace riffra
