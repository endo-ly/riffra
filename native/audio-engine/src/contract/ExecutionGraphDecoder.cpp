#include "ExecutionGraphDecoder.h"

#include <cmath>
#include <limits>

#include "ContractReader.h"

namespace riffra {
namespace {

bool fail(juce::String& error, const juce::String& path, const juce::String& message) {
    if (error.isEmpty()) error = path + ": " + message;
    return false;
}

juce::String child(const juce::String& path, const char* key) {
    return path.isEmpty() ? juce::String(key) : path + "." + key;
}

juce::String element(const juce::String& path, const int index) {
    return path + "[" + juce::String(index) + "]";
}

juce::String unknownValue(const juce::String& value) { return "unknown value \"" + value + "\""; }

bool readEnum(const juce::String& value, const juce::String& path,
              std::initializer_list<std::pair<const char*, int>> values, int& output,
              juce::String& error) {
    for (const auto& [name, mapped] : values) {
        if (value == name) {
            output = mapped;
            return true;
        }
    }
    return fail(error, path, unknownValue(value));
}

bool decodeTimebase(const juce::var& value, const juce::String& path, TimebaseSpec& output,
                    juce::String& error) {
    ContractReader reader(
        value, path, {"ppq", "bpm", "timeSignatureNumerator", "timeSignatureDenominator"}, error);
    if (!reader.unsigned32("ppq", output.ppq) || !reader.number("bpm", output.bpm) ||
        !reader.unsigned8("timeSignatureNumerator", output.timeSignatureNumerator) ||
        !reader.unsigned8("timeSignatureDenominator", output.timeSignatureDenominator) ||
        !reader.finish())
        return false;
    if (output.ppq != 960) return fail(error, child(path, "ppq"), "must equal 960");
    if (output.bpm < 20.0 || output.bpm > 400.0)
        return fail(error, child(path, "bpm"), "must be between 20 and 400");
    if (output.timeSignatureNumerator == 0)
        return fail(error, child(path, "timeSignatureNumerator"), "must be at least 1");
    if (output.timeSignatureDenominator == 0)
        return fail(error, child(path, "timeSignatureDenominator"), "must be at least 1");
    return true;
}

bool decodeLoopRange(const juce::var& value, const juce::String& path, LoopRangeSpec& output,
                     juce::String& error) {
    ContractReader reader(value, path, {"enabled", "startTick", "endTick"}, error);
    if (!reader.boolean("enabled", output.enabled) ||
        !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("endTick", output.endTick) || !reader.finish())
        return false;
    if (output.enabled && output.endTick <= output.startTick)
        return fail(error, child(path, "endTick"), "must be greater than startTick when enabled");
    return true;
}

bool decodeTickRange(const juce::var& value, const juce::String& path, TickRangeSpec& output,
                     juce::String& error) {
    ContractReader reader(value, path, {"startTick", "endTick"}, error);
    if (!reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("endTick", output.endTick) || !reader.finish())
        return false;
    if (output.endTick <= output.startTick)
        return fail(error, child(path, "endTick"), "must be greater than startTick");
    return true;
}

bool decodeAutomation(const juce::var& value, const juce::String& path, const bool volume,
                      std::vector<AutomationPointSpec>& output, juce::String& error) {
    ContractReader reader(value, path, {"tick", "value"}, error);
    AutomationPointSpec point;
    if (!reader.unsignedInteger("tick", point.tick) || !reader.number("value", point.value) ||
        !reader.finish())
        return false;
    const auto minimum = volume ? -90.0 : -1.0;
    const auto maximum = volume ? 24.0 : 1.0;
    if (point.value < minimum || point.value > maximum)
        return fail(error, child(path, "value"),
                    "must be between " + juce::String(minimum) + " and " + juce::String(maximum));
    output.push_back(point);
    return true;
}

bool decodeAutomationList(const juce::Array<juce::var>& values, const juce::String& path,
                          const bool volume, std::vector<AutomationPointSpec>& output,
                          juce::String& error) {
    output.clear();
    output.reserve(static_cast<std::size_t>(values.size()));
    for (int index = 0; index < values.size(); ++index) {
        if (!decodeAutomation(values.getReference(index), element(path, index), volume, output,
                              error))
            return false;
    }
    return true;
}

bool decodePluginStateValue(const juce::var& value, const juce::String& path,
                            PluginStateSpec& output, juce::String& error) {
    ContractReader reader(value, path, {"stateData", "parameterValues", "bypassed"}, error);
    juce::Array<juce::var> parameters;
    if (!reader.optionalString("stateData", output.stateData) ||
        !reader.array("parameterValues", parameters) ||
        !reader.boolean("bypassed", output.bypassed) || !reader.finish())
        return false;
    output.parameterValues.clear();
    output.parameterValues.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        const auto item = parameters.getReference(index);
        if ((!item.isDouble() && !item.isInt() && !item.isInt64()) ||
            !std::isfinite(static_cast<double>(item)))
            return fail(error, element(child(path, "parameterValues"), index),
                        "expected finite number");
        const auto converted = static_cast<float>(static_cast<double>(item));
        if (!std::isfinite(converted))
            return fail(error, element(child(path, "parameterValues"), index),
                        "number exceeds float range");
        output.parameterValues.push_back(converted);
    }
    return true;
}

bool decodePluginDevice(const juce::var& value, const juce::String& path, PluginDeviceSpec& output,
                        juce::String& error) {
    ContractReader reader(value, path, {"id", "path", "state"}, error);
    juce::var state;
    if (!reader.string("id", output.id) || !reader.string("path", output.path) ||
        !reader.object("state", state) || !reader.finish())
        return false;
    return decodePluginStateValue(state, child(path, "state"), output.state, error);
}

bool decodeInstrument(const juce::var& value, const juce::String& path, InstrumentSpec& output,
                      juce::String& error) {
    ContractReader tagReader(value, path, {"type"}, error);
    juce::String type;
    if (!tagReader.string("type", type)) return false;
    if (type == "vst3") {
        ContractReader reader(value, path, {"type", "id", "path", "state"}, error);
        juce::String actualType;
        Vst3InstrumentSpec instrument;
        juce::var state;
        if (!reader.string("type", actualType) || actualType != type ||
            !reader.string("id", instrument.id) || !reader.string("path", instrument.path) ||
            !reader.object("state", state) || !reader.finish())
            return false;
        if (!decodePluginStateValue(state, child(path, "state"), instrument.state, error))
            return false;
        output = std::move(instrument);
        return true;
    }
    if (type == "internal") {
        ContractReader reader(
            value, path, {"type", "id", "bypassed", "definitionJson", "definitionBaseDir"}, error);
        juce::String actualType;
        InternalInstrumentSpec instrument;
        if (!reader.string("type", actualType) || actualType != type ||
            !reader.string("id", instrument.id) ||
            !reader.boolean("bypassed", instrument.bypassed) ||
            !reader.string("definitionJson", instrument.definitionJson) ||
            !reader.string("definitionBaseDir", instrument.definitionBaseDir) || !reader.finish())
            return false;
        output = std::move(instrument);
        return true;
    }
    return fail(error, child(path, "type"), unknownValue(type));
}

bool decodeAudioClip(const juce::var& value, const juce::String& path, AudioClipSpec& output,
                     juce::String& error) {
    ContractReader reader(
        value, path,
        {"id", "path", "sourceSampleRate", "sourceStartFrame", "sourceEndFrame", "durationFrames",
         "durationSampleRate", "startTick", "fadeInFrames", "fadeOutFrames", "fadeShape", "gainDb",
         "pan", "takeVariant", "loopEnabled", "muted"},
        error);
    juce::String fadeShape;
    juce::String takeVariant;
    if (!reader.string("id", output.id) || !reader.string("path", output.path) ||
        !reader.unsigned32("sourceSampleRate", output.sourceSampleRate) ||
        !reader.unsignedInteger("sourceStartFrame", output.sourceStartFrame) ||
        !reader.unsignedInteger("sourceEndFrame", output.sourceEndFrame) ||
        !reader.unsignedInteger("durationFrames", output.durationFrames) ||
        !reader.unsigned32("durationSampleRate", output.durationSampleRate) ||
        !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("fadeInFrames", output.fadeInFrames) ||
        !reader.unsignedInteger("fadeOutFrames", output.fadeOutFrames) ||
        !reader.string("fadeShape", fadeShape) || !reader.number("gainDb", output.gainDb) ||
        !reader.number("pan", output.pan) || !reader.string("takeVariant", takeVariant) ||
        !reader.boolean("loopEnabled", output.loopEnabled) ||
        !reader.boolean("muted", output.muted) || !reader.finish())
        return false;
    int fade = 0;
    int variant = 0;
    if (!readEnum(fadeShape, child(path, "fadeShape"),
                  {{"linear", 0}, {"equalPower", 1}, {"smooth", 2}}, fade, error) ||
        !readEnum(takeVariant, child(path, "takeVariant"), {{"raw", 0}, {"processed", 1}}, variant,
                  error))
        return false;
    output.fadeShape = static_cast<FadeShapeSpec>(fade);
    output.takeVariant = static_cast<TakeVariantSpec>(variant);
    if (output.sourceSampleRate == 0)
        return fail(error, child(path, "sourceSampleRate"), "must be greater than 0");
    if (output.durationSampleRate == 0)
        return fail(error, child(path, "durationSampleRate"), "must be greater than 0");
    if (output.sourceEndFrame <= output.sourceStartFrame)
        return fail(error, child(path, "sourceEndFrame"), "must be greater than sourceStartFrame");
    if (output.durationFrames == 0)
        return fail(error, child(path, "durationFrames"), "must be greater than 0");
    if (output.gainDb < -90.0 || output.gainDb > 24.0)
        return fail(error, child(path, "gainDb"), "must be between -90 and 24");
    if (output.pan < -1.0 || output.pan > 1.0)
        return fail(error, child(path, "pan"), "must be between -1 and 1");
    return true;
}

bool decodeMidiNote(const juce::var& value, const juce::String& path, MidiNoteSpec& output,
                    juce::String& error) {
    ContractReader reader(value, path,
                          {"startTick", "durationTicks", "note", "velocity", "channel"}, error);
    if (!reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("durationTicks", output.durationTicks) ||
        !reader.unsigned8("note", output.note) || !reader.unsigned8("velocity", output.velocity) ||
        !reader.unsigned8("channel", output.channel) || !reader.finish())
        return false;
    if (output.durationTicks == 0)
        return fail(error, child(path, "durationTicks"), "must be greater than 0");
    if (output.note > 127) return fail(error, child(path, "note"), "must be in 0..=127");
    if (output.velocity == 0 || output.velocity > 127)
        return fail(error, child(path, "velocity"), "must be in 1..=127");
    if (output.channel < 1 || output.channel > 16)
        return fail(error, child(path, "channel"), "must be in 1..=16");
    return true;
}

bool decodeMidiEvent(const juce::var& value, const juce::String& path, MidiEventSpec& output,
                     juce::String& error) {
    ContractReader reader(value, path, {"kind", "tick", "channel", "data1", "data2"}, error);
    juce::String kind;
    if (!reader.string("kind", kind) || !reader.unsignedInteger("tick", output.tick) ||
        !reader.unsigned8("channel", output.channel) || !reader.unsigned8("data1", output.data1) ||
        !reader.unsigned8("data2", output.data2) || !reader.finish())
        return false;
    int mapped = 0;
    if (!readEnum(kind, child(path, "kind"),
                  {{"controlChange", 0}, {"pitchBend", 1}, {"channelPressure", 2}}, mapped, error))
        return false;
    output.kind = static_cast<MidiEventKindSpec>(mapped);
    if (output.channel < 1 || output.channel > 16)
        return fail(error, child(path, "channel"), "must be in 1..=16");
    if (output.data1 > 127) return fail(error, child(path, "data1"), "must be in 0..=127");
    if (output.data2 > 127) return fail(error, child(path, "data2"), "must be in 0..=127");
    return true;
}

bool decodeMidiClip(const juce::var& value, const juce::String& path, MidiClipSpec& output,
                    juce::String& error) {
    ContractReader reader(
        value, path,
        {"id", "startTick", "durationTicks", "loopEnabled", "muted", "notes", "events"}, error);
    juce::Array<juce::var> notes;
    juce::Array<juce::var> events;
    if (!reader.string("id", output.id) || !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("durationTicks", output.durationTicks) ||
        !reader.boolean("loopEnabled", output.loopEnabled) ||
        !reader.boolean("muted", output.muted) || !reader.array("notes", notes) ||
        !reader.array("events", events) || !reader.finish())
        return false;
    if (output.durationTicks == 0)
        return fail(error, child(path, "durationTicks"), "must be greater than 0");
    for (int index = 0; index < notes.size(); ++index) {
        MidiNoteSpec note;
        const auto notePath = element(child(path, "notes"), index);
        if (!decodeMidiNote(notes.getReference(index), notePath, note, error)) return false;
        if (note.startTick >= output.durationTicks)
            return fail(error, child(notePath, "startTick"),
                        "must be less than clip durationTicks");
        output.notes.push_back(note);
    }
    for (int index = 0; index < events.size(); ++index) {
        MidiEventSpec event;
        const auto eventPath = element(child(path, "events"), index);
        if (!decodeMidiEvent(events.getReference(index), eventPath, event, error)) return false;
        if (event.tick >= output.durationTicks)
            return fail(error, child(eventPath, "tick"), "must be less than clip durationTicks");
        output.events.push_back(event);
    }
    return true;
}

bool decodeTrack(const juce::var& value, const juce::String& path, TrackSpec& output,
                 juce::String& error) {
    ContractReader reader(value, path,
                          {"id", "kind", "gainDb", "pan", "muted", "solo", "armed", "monitorInput",
                           "audioInput", "midiInput", "volumeAutomation", "panAutomation",
                           "effects", "instrument", "audioClips", "midiClips"},
                          error);
    juce::String kind;
    juce::var audioInput;
    juce::var midiInput;
    juce::var instrument;
    juce::Array<juce::var> volumeAutomation;
    juce::Array<juce::var> panAutomation;
    juce::Array<juce::var> effects;
    juce::Array<juce::var> audioClips;
    juce::Array<juce::var> midiClips;
    if (!reader.string("id", output.id) || !reader.string("kind", kind) ||
        !reader.number("gainDb", output.gainDb) || !reader.number("pan", output.pan) ||
        !reader.boolean("muted", output.muted) || !reader.boolean("solo", output.solo) ||
        !reader.boolean("armed", output.armed) ||
        !reader.boolean("monitorInput", output.monitorInput) ||
        !reader.value("audioInput", audioInput) || !reader.object("midiInput", midiInput) ||
        !reader.array("volumeAutomation", volumeAutomation) ||
        !reader.array("panAutomation", panAutomation) || !reader.array("effects", effects) ||
        !reader.value("instrument", instrument) || !reader.array("audioClips", audioClips) ||
        !reader.array("midiClips", midiClips) || !reader.finish())
        return false;
    int trackKind = 0;
    if (!readEnum(kind, child(path, "kind"), {{"audio", 0}, {"instrument", 1}}, trackKind, error))
        return false;
    output.kind = static_cast<TrackKindSpec>(trackKind);
    if (output.gainDb < -90.0 || output.gainDb > 24.0)
        return fail(error, child(path, "gainDb"), "must be between -90 and 24");
    if (output.pan < -1.0 || output.pan > 1.0)
        return fail(error, child(path, "pan"), "must be between -1 and 1");

    if (!audioInput.isVoid()) {
        AudioInputSpec input;
        ContractReader inputReader(audioInput, child(path, "audioInput"), {"channelIndex"}, error);
        if (!inputReader.unsigned32("channelIndex", input.channelIndex) || !inputReader.finish())
            return false;
        if (input.channelIndex >= 32)
            return fail(error, child(child(path, "audioInput"), "channelIndex"),
                        "must be less than 32");
        output.audioInput = input;
    }

    ContractReader midiReader(midiInput, child(path, "midiInput"), {"deviceId", "channel"}, error);
    if (!midiReader.optionalString("deviceId", output.midiInput.deviceId) ||
        !midiReader.optionalUnsigned8("channel", output.midiInput.channel) || !midiReader.finish())
        return false;
    if (output.midiInput.channel.has_value() &&
        (*output.midiInput.channel < 1 || *output.midiInput.channel > 16))
        return fail(error, child(child(path, "midiInput"), "channel"), "must be in 1..=16");

    if (!decodeAutomationList(volumeAutomation, child(path, "volumeAutomation"), true,
                              output.volumeAutomation, error) ||
        !decodeAutomationList(panAutomation, child(path, "panAutomation"), false,
                              output.panAutomation, error))
        return false;
    for (int index = 0; index < effects.size(); ++index) {
        PluginDeviceSpec device;
        if (!decodePluginDevice(effects.getReference(index), element(child(path, "effects"), index),
                                device, error))
            return false;
        output.effects.push_back(std::move(device));
    }
    if (!instrument.isVoid()) {
        InstrumentSpec decoded;
        if (!decodeInstrument(instrument, child(path, "instrument"), decoded, error)) return false;
        output.instrument = std::move(decoded);
    }
    for (int index = 0; index < audioClips.size(); ++index) {
        AudioClipSpec clip;
        if (!decodeAudioClip(audioClips.getReference(index),
                             element(child(path, "audioClips"), index), clip, error))
            return false;
        output.audioClips.push_back(std::move(clip));
    }
    for (int index = 0; index < midiClips.size(); ++index) {
        MidiClipSpec clip;
        if (!decodeMidiClip(midiClips.getReference(index), element(child(path, "midiClips"), index),
                            clip, error))
            return false;
        output.midiClips.push_back(std::move(clip));
    }
    return true;
}

bool decodeGraph(const juce::var& value, const juce::String& path, ExecutionGraph& output,
                 juce::String& error) {
    ContractReader reader(
        value, path,
        {"timebase", "loopRange", "punchRange", "metronomeEnabled", "masterGainDb", "tracks"},
        error);
    juce::var timebase;
    juce::var loopRange;
    juce::var punchRange;
    juce::Array<juce::var> tracks;
    if (!reader.object("timebase", timebase) || !reader.object("loopRange", loopRange) ||
        !reader.value("punchRange", punchRange) ||
        !reader.boolean("metronomeEnabled", output.metronomeEnabled) ||
        !reader.number("masterGainDb", output.masterGainDb) || !reader.array("tracks", tracks) ||
        !reader.finish())
        return false;
    if (!decodeTimebase(timebase, child(path, "timebase"), output.timebase, error) ||
        !decodeLoopRange(loopRange, child(path, "loopRange"), output.loopRange, error))
        return false;
    if (!punchRange.isVoid()) {
        TickRangeSpec punch;
        if (!decodeTickRange(punchRange, child(path, "punchRange"), punch, error)) return false;
        output.punchRange = punch;
    }
    if (output.masterGainDb < -90.0 || output.masterGainDb > 0.0)
        return fail(error, child(path, "masterGainDb"), "must be between -90 and 0");
    output.tracks.reserve(static_cast<std::size_t>(tracks.size()));
    for (int index = 0; index < tracks.size(); ++index) {
        TrackSpec track;
        if (!decodeTrack(tracks.getReference(index), element(child(path, "tracks"), index), track,
                         error))
            return false;
        output.tracks.push_back(std::move(track));
    }
    return true;
}

}  // namespace

bool decodePluginState(const juce::var& value, PluginStateSpec& output, juce::String& error) {
    return decodePluginStateValue(value, "pluginState", output, error);
}

bool decodeTimelineSnapshot(const juce::var& value, TimelineSnapshotSpec& output,
                            juce::String& error) {
    ContractReader reader(value, "snapshot", {"projectId", "revision", "graph"}, error);
    juce::var graph;
    if (!reader.string("projectId", output.projectId) ||
        !reader.unsignedInteger("revision", output.revision) || !reader.object("graph", graph) ||
        !reader.finish())
        return false;
    return decodeGraph(graph, "snapshot.graph", output.graph, error);
}

bool decodeOfflineRenderRequest(const juce::var& value, OfflineRenderRequestSpec& output,
                                juce::String& error) {
    ContractReader reader(
        value, "request",
        {"graph", "destination", "startTick", "endTick", "sampleRate", "blockSize", "normalize"},
        error);
    juce::var graph;
    if (!reader.object("graph", graph) || !reader.string("destination", output.destination) ||
        !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("endTick", output.endTick) ||
        !reader.unsigned32("sampleRate", output.sampleRate) ||
        !reader.unsigned32("blockSize", output.blockSize) ||
        !reader.boolean("normalize", output.normalize) || !reader.finish())
        return false;
    if (!decodeGraph(graph, "request.graph", output.graph, error)) return false;
    if (output.endTick <= output.startTick)
        return fail(error, "request.endTick", "must be greater than startTick");
    if (output.destination.isEmpty())
        return fail(error, "request.destination", "must not be empty");
    if (output.sampleRate == 0) return fail(error, "request.sampleRate", "must be greater than 0");
    if (output.blockSize == 0) return fail(error, "request.blockSize", "must be greater than 0");
    if (output.blockSize > static_cast<std::uint32_t>(std::numeric_limits<int>::max()))
        return fail(error, "request.blockSize", "exceeds the native block size range");
    return true;
}

}  // namespace riffra
