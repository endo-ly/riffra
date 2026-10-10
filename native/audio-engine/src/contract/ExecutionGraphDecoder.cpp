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
    ContractReader reader(value, path, {"ppq", "tempoChanges", "timeSignatureChanges"}, error);
    juce::Array<juce::var> tempos, signatures;
    if (!reader.unsigned32("ppq", output.ppq) || !reader.array("tempoChanges", tempos) ||
        !reader.array("timeSignatureChanges", signatures) || !reader.finish())
        return false;
    if (output.ppq != 960) return fail(error, child(path, "ppq"), "must equal 960");
    output.tempoChanges.clear();
    output.timeSignatureChanges.clear();
    for (int i = 0; i < tempos.size(); ++i) {
        const auto location = element(child(path, "tempoChanges"), i);
        ContractReader point(tempos[i], location, {"tick", "bpm"}, error);
        TempoChangeSpec change;
        if (!point.unsignedInteger("tick", change.tick) || !point.number("bpm", change.bpm) ||
            !point.finish())
            return false;
        if (change.bpm <= 0.0 ||
            (i == 0 ? change.tick != 0 : change.tick <= output.tempoChanges.back().tick))
            return fail(error, location,
                        "expected positive tempo and strictly sorted changes starting at zero");
        output.tempoChanges.push_back(change);
    }
    for (int i = 0; i < signatures.size(); ++i) {
        const auto location = element(child(path, "timeSignatureChanges"), i);
        ContractReader point(signatures[i], location, {"tick", "numerator", "denominator"}, error);
        TimeSignatureChangeSpec change;
        if (!point.unsignedInteger("tick", change.tick) ||
            !point.unsigned8("numerator", change.numerator) ||
            !point.unsigned8("denominator", change.denominator) || !point.finish())
            return false;
        if (change.numerator == 0 ||
            (change.denominator != 1 && change.denominator != 2 && change.denominator != 4 &&
             change.denominator != 8 && change.denominator != 16 && change.denominator != 32) ||
            (i == 0 ? change.tick != 0 : change.tick <= output.timeSignatureChanges.back().tick))
            return fail(error, location,
                        "expected valid meter and strictly sorted changes starting at zero");
        output.timeSignatureChanges.push_back(change);
    }
    if (output.tempoChanges.empty() || output.timeSignatureChanges.empty())
        return fail(error, path, "timebase maps must not be empty");
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

bool decodeInstrumentControlEvent(const juce::var& value, const juce::String& path,
                                  InstrumentControlEventSpec& output, juce::String& error) {
    ContractReader reader(value, path, {"id", "tick", "sourceOrder", "kind"}, error);
    juce::var kind;
    if (!reader.string("id", output.id) || !reader.unsignedInteger("tick", output.tick) ||
        !reader.unsigned32("sourceOrder", output.sourceOrder) || !reader.object("kind", kind) ||
        !reader.finish())
        return false;
    output.type = kind["type"].toString();
    const auto kindPath = child(path, "kind");
    juce::String type;
    double number = 0.0;
    if (output.type == "sustainPedal") {
        ContractReader control(kind, kindPath, {"type", "down"}, error);
        return control.string("type", type) && control.boolean("down", output.down) &&
               control.finish();
    }
    if (output.type == "parameterChange") {
        ContractReader control(kind, kindPath, {"type", "parameter", "nativeValue"}, error);
        if (!control.string("type", type) || !control.string("parameter", output.parameter) ||
            !control.number("nativeValue", number) || !control.finish())
            return false;
        if (output.parameter.isEmpty()) return fail(error, kindPath, "parameter must not be empty");
    } else {
        if (output.type != "pitchBend" && output.type != "modWheel" && output.type != "aftertouch")
            return fail(error, kindPath, "unknown instrument control type");
        ContractReader control(kind, kindPath, {"type", "value"}, error);
        if (!control.string("type", type) || !control.number("value", number) || !control.finish())
            return false;
        if (number < (output.type == "pitchBend" ? -1.0 : 0.0) || number > 1.0)
            return fail(error, kindPath, "control value is out of range");
    }
    output.value = static_cast<float>(number);
    if (!std::isfinite(output.value))
        return fail(error, kindPath, "value cannot be represented as f32");
    return true;
}

bool decodeMidiClip(const juce::var& value, const juce::String& path, MidiClipSpec& output,
                    juce::String& error) {
    ContractReader reader(value, path,
                          {"id", "startTick", "durationTicks", "loopEnabled", "muted", "notes",
                           "events", "instrumentControlEvents"},
                          error);
    juce::Array<juce::var> notes;
    juce::Array<juce::var> events;
    juce::Array<juce::var> controls;
    if (!reader.string("id", output.id) || !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("durationTicks", output.durationTicks) ||
        !reader.boolean("loopEnabled", output.loopEnabled) ||
        !reader.boolean("muted", output.muted) || !reader.array("notes", notes) ||
        !reader.array("events", events) || !reader.array("instrumentControlEvents", controls) ||
        !reader.finish())
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
    for (int index = 0; index < controls.size(); ++index) {
        InstrumentControlEventSpec control;
        const auto eventPath = element(child(path, "instrumentControlEvents"), index);
        if (!decodeInstrumentControlEvent(controls.getReference(index), eventPath, control, error))
            return false;
        if (control.tick > output.durationTicks)
            return fail(error, child(eventPath, "tick"), "exceeds clip durationTicks");
        output.instrumentControlEvents.push_back(std::move(control));
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
    ContractReader reader(
        value, path,
        {"id", "kind", "gainDb", "pan", "muted", "solo", "armed", "monitorInput", "audioInput",
         "midiInput", "volumeAutomation", "panAutomation", "effects", "instrument", "audioClips",
         "midiClips", "panLaw", "externalAudioSourceTrackId"},
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
        !reader.string("panLaw", output.panLaw) ||
        !reader.optionalString("externalAudioSourceTrackId", output.externalAudioSourceTrackId) ||
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
    if (output.panLaw != "equalPower" && output.panLaw != "unityCenterStereo")
        return fail(error, child(path, "panLaw"), "unknown pan law");
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
    ContractReader reader(value, path,
                          {"timebase", "loopRange", "punchRange", "metronomeEnabled",
                           "masterGainDb", "tracks", "mixdown"},
                          error);
    juce::var timebase;
    juce::var loopRange;
    juce::var punchRange;
    juce::var mixdown;
    juce::Array<juce::var> tracks;
    if (!reader.object("timebase", timebase) || !reader.object("loopRange", loopRange) ||
        !reader.value("punchRange", punchRange) ||
        !reader.boolean("metronomeEnabled", output.metronomeEnabled) ||
        !reader.number("masterGainDb", output.masterGainDb) || !reader.array("tracks", tracks) ||
        !reader.object("mixdown", mixdown) || !reader.finish())
        return false;
    ContractReader mixReader(mixdown, child(path, "mixdown"),
                             {"musicalEndTick", "tailSeconds", "fadeOutSeconds"}, error);
    if (!mixReader.unsignedInteger("musicalEndTick", output.musicalEndTick) ||
        !mixReader.number("tailSeconds", output.tailSeconds) ||
        !mixReader.number("fadeOutSeconds", output.fadeOutSeconds) || !mixReader.finish())
        return false;
    if (output.tailSeconds < 0.0 || output.fadeOutSeconds < 0.0)
        return fail(error, child(path, "mixdown"), "durations must be nonnegative");
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
    ContractReader reader(value, "request",
                          {"graph", "destination", "startTick", "endTick", "sampleRate",
                           "blockSize", "normalize", "tailSeconds", "includeEndEvents"},
                          error);
    juce::var graph;
    if (!reader.object("graph", graph) || !reader.string("destination", output.destination) ||
        !reader.unsignedInteger("startTick", output.startTick) ||
        !reader.unsignedInteger("endTick", output.endTick) ||
        !reader.unsigned32("sampleRate", output.sampleRate) ||
        !reader.unsigned32("blockSize", output.blockSize) ||
        !reader.number("tailSeconds", output.tailSeconds) ||
        !reader.boolean("includeEndEvents", output.includeEndEvents) ||
        !reader.boolean("normalize", output.normalize) || !reader.finish())
        return false;
    if (!decodeGraph(graph, "request.graph", output.graph, error)) return false;
    if (output.tailSeconds < 0.0) return fail(error, "request.tailSeconds", "must be nonnegative");
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
