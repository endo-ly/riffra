#include "ExecutionGraphDecoder.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <set>

#include "ContractReader.h"
#include "MixerGraphValidation.h"

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
    if (output.ppq == 0) return fail(error, child(path, "ppq"), "must be positive");
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

bool decodeAutomationLane(const juce::var& value, const juce::String& path,
                          AutomationLaneSpec& output, juce::String& error) {
    ContractReader reader(value, path, {"target", "interpolation", "points"}, error);
    juce::var target;
    juce::var interpolation;
    juce::Array<juce::var> points;
    if (!reader.object("target", target) || !reader.value("interpolation", interpolation) ||
        !reader.array("points", points) || !reader.finish())
        return false;
    const auto targetPath = child(path, "target");
    const auto kind = target.getProperty("kind", {}).toString();
    juce::String parameter;
    juce::String decodedKind;
    if (kind == "trackMix" || kind == "busMix") {
        const auto track = kind == "trackMix";
        ContractReader t(target, targetPath, {"kind", track ? "trackId" : "busId", "parameter"},
                         error);
        if (!t.string("kind", decodedKind) || !t.string(track ? "trackId" : "busId", output.id) ||
            !t.string("parameter", parameter) || !t.finish())
            return false;
        if (track && parameter == "volume")
            output.kind = AutomationTargetKind::trackVolume;
        else if (track && parameter == "pan")
            output.kind = AutomationTargetKind::trackPan;
        else if (!track && parameter == "gain")
            output.kind = AutomationTargetKind::busGain;
        else if (!track && parameter == "pan")
            output.kind = AutomationTargetKind::busPan;
        else
            return fail(error, targetPath, "unknown mix parameter");
    } else if (kind == "masterGain") {
        ContractReader t(target, targetPath, {"kind"}, error);
        if (!t.string("kind", decodedKind) || !t.finish()) return false;
        output.kind = AutomationTargetKind::masterGain;
    } else if (kind == "sendLevel") {
        ContractReader t(target, targetPath, {"kind", "sendId"}, error);
        if (!t.string("kind", decodedKind) || !t.string("sendId", output.id) || !t.finish())
            return false;
        output.kind = AutomationTargetKind::sendLevel;
    } else if (kind == "sidechainSendLevel") {
        ContractReader t(target, targetPath, {"kind", "sidechainSendId"}, error);
        if (!t.string("kind", decodedKind) || !t.string("sidechainSendId", output.id) ||
            !t.finish())
            return false;
        output.kind = AutomationTargetKind::sidechainSendLevel;
    } else if (kind == "deviceParameter") {
        ContractReader t(target, targetPath, {"kind", "deviceId", "parameterId"}, error);
        if (!t.string("kind", decodedKind) || !t.string("deviceId", output.id) ||
            !t.string("parameterId", output.parameterId) || !t.finish())
            return false;
        output.kind = AutomationTargetKind::deviceParameter;
        if (output.parameterId.isEmpty())
            return fail(error, targetPath, "parameter ID must be non-empty");
    } else
        return fail(error, targetPath, "unknown automation target");
    if (output.kind != AutomationTargetKind::masterGain && output.id.isEmpty())
        return fail(error, targetPath, "target ID must be non-empty");
    if (interpolation.isObject()) {
        ContractReader mode(interpolation, child(path, "interpolation"), {"linearAmplitude"},
                            error);
        juce::var transition;
        if (!mode.object("linearAmplitude", transition) || !mode.finish()) return false;
        ContractReader duration(transition, child(path, "interpolation.linearAmplitude"),
                                {"durationSeconds"}, error);
        if (!duration.number("durationSeconds", output.transitionSeconds) || !duration.finish())
            return false;
        if (output.transitionSeconds <= 0.0 || output.kind != AutomationTargetKind::trackVolume)
            return fail(error, path, "invalid amplitude transition");
        output.interpolation = AutomationInterpolationSpec::linearAmplitude;
    } else {
        int mode = 0;
        if (!readEnum(interpolation.toString(), child(path, "interpolation"),
                      {{"linear", 0}, {"step", 1}}, mode, error))
            return false;
        output.interpolation = static_cast<AutomationInterpolationSpec>(mode);
    }
    if (points.isEmpty() || points.size() > 16384)
        return fail(error, child(path, "points"), "requires 1..16384 points");
    const auto pan = output.kind == AutomationTargetKind::trackPan ||
                     output.kind == AutomationTargetKind::busPan;
    const auto device = output.kind == AutomationTargetKind::deviceParameter;
    const auto volume = output.kind == AutomationTargetKind::trackVolume;
    const auto minimum = pan      ? -1.0
                         : device ? 0.0
                         : volume ? -std::numeric_limits<double>::max()
                                  : -90.0;
    const auto maximum = pan || device                                     ? 1.0
                         : output.kind == AutomationTargetKind::masterGain ? 0.0
                         : volume ? 20.0 * std::log10(std::numeric_limits<float>::max())
                                  : 24.0;
    for (int index = 0; index < points.size(); ++index) {
        const auto pointPath = element(child(path, "points"), index);
        ContractReader p(points.getReference(index), pointPath, {"tick", "value"}, error);
        AutomationPointSpec point;
        if (!p.unsignedInteger("tick", point.tick) || !p.number("value", point.value) ||
            !p.finish())
            return false;
        if (point.value < minimum || point.value > maximum)
            return fail(error, child(pointPath, "value"), "outside target value range");
        if (!output.points.empty() && point.tick <= output.points.back().tick)
            return fail(error, child(pointPath, "tick"), "ticks must be strictly increasing");
        output.points.push_back(point);
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

bool decodeMidiNoteArray(const juce::Array<juce::var>& source, const juce::String& path,
                         std::vector<std::uint8_t>& output, juce::String& error) {
    std::set<std::uint8_t> unique;
    output.clear();
    output.reserve(static_cast<std::size_t>(source.size()));
    for (int index = 0; index < source.size(); ++index) {
        const auto& value = source[index];
        const std::int64_t note = value.isInt()     ? static_cast<int>(value)
                                  : value.isInt64() ? static_cast<juce::int64>(value)
                                                    : -1;
        if (note < 0 || note > 127)
            return fail(error, element(path, index), "must be an integer in 0..=127");
        const auto decoded = static_cast<std::uint8_t>(note);
        if (!unique.insert(decoded).second)
            return fail(error, element(path, index), "must not contain duplicates");
        output.push_back(decoded);
    }
    if (output.empty()) return fail(error, path, "must not be empty");
    return true;
}

bool decodeInstrumentMidi(const juce::var& value, const juce::String& path,
                          InstrumentMidiSpec& output, juce::String& error) {
    ContractReader reader(value, path, {"transposeSemitones", "playableNotes", "keyswitches"},
                          error);
    double transpose = 0.0;
    juce::var playable;
    juce::Array<juce::var> keyswitches;
    if (!reader.number("transposeSemitones", transpose) ||
        !reader.value("playableNotes", playable) || !reader.array("keyswitches", keyswitches) ||
        !reader.finish())
        return false;
    if (std::floor(transpose) != transpose || transpose < -127 || transpose > 127)
        return fail(error, child(path, "transposeSemitones"), "must be an integer in -127..=127");
    output.transposeSemitones = static_cast<std::int16_t>(transpose);

    if (!playable.isVoid()) {
        juce::String type;
        ContractReader tagReader(playable, child(path, "playableNotes"), {"type"}, error);
        if (!tagReader.string("type", type)) return false;
        if (type == "range") {
            ContractReader rangeReader(playable, child(path, "playableNotes"),
                                       {"type", "start", "end"}, error);
            PlayableNotesSpec range;
            range.kind = PlayableNotesKindSpec::range;
            juce::String actualType;
            if (!rangeReader.string("type", actualType) || actualType != type ||
                !rangeReader.unsigned8("start", range.start) ||
                !rangeReader.unsigned8("end", range.end) || !rangeReader.finish())
                return false;
            if (range.start > 127 || range.end > 127)
                return fail(error, child(path, "playableNotes"),
                            "must be within MIDI notes 0..=127");
            if (range.start > range.end)
                return fail(error, child(child(path, "playableNotes"), "start"),
                            "must not exceed end");
            output.playableNotes = range;
        } else if (type == "set") {
            ContractReader setReader(playable, child(path, "playableNotes"), {"type", "notes"},
                                     error);
            juce::String actualType;
            juce::Array<juce::var> notes;
            if (!setReader.string("type", actualType) || actualType != type ||
                !setReader.array("notes", notes) || !setReader.finish())
                return false;
            PlayableNotesSpec set;
            set.kind = PlayableNotesKindSpec::set;
            if (!decodeMidiNoteArray(notes, child(child(path, "playableNotes"), "notes"), set.notes,
                                     error))
                return false;
            output.playableNotes = std::move(set);
        } else {
            return fail(error, child(child(path, "playableNotes"), "type"), unknownValue(type));
        }
    }

    output.keyswitches.clear();
    std::set<std::uint8_t> uniqueKeyswitches;
    output.keyswitches.reserve(static_cast<std::size_t>(keyswitches.size()));
    for (int index = 0; index < keyswitches.size(); ++index) {
        const auto itemPath = element(child(path, "keyswitches"), index);
        ContractReader keyReader(keyswitches[index], itemPath, {"note", "name"}, error);
        InstrumentKeyswitchSpec key;
        if (!keyReader.unsigned8("note", key.note) || !keyReader.string("name", key.name) ||
            !keyReader.finish())
            return false;
        if (key.note > 127) return fail(error, child(itemPath, "note"), "must be in 0..=127");
        key.name = key.name.trim();
        if (key.name.isEmpty()) return fail(error, child(itemPath, "name"), "must not be empty");
        if (!uniqueKeyswitches.insert(key.note).second)
            return fail(error, child(itemPath, "note"), "must not contain duplicates");
        output.keyswitches.push_back(std::move(key));
    }
    return true;
}

bool decodeInstrument(const juce::var& value, const juce::String& path, InstrumentSpec& output,
                      juce::String& error) {
    ContractReader tagReader(value, path, {"type"}, error);
    juce::String type;
    if (!tagReader.string("type", type)) return false;
    if (type == "vst3") {
        ContractReader reader(value, path, {"type", "id", "path", "state", "midi"}, error);
        juce::String actualType;
        Vst3InstrumentSpec instrument;
        juce::var state, midi;
        if (!reader.string("type", actualType) || actualType != type ||
            !reader.string("id", instrument.id) || !reader.string("path", instrument.path) ||
            !reader.object("state", state) || !reader.object("midi", midi) || !reader.finish())
            return false;
        if (!decodePluginStateValue(state, child(path, "state"), instrument.state, error))
            return false;
        if (!decodeInstrumentMidi(midi, child(path, "midi"), instrument.midi, error)) return false;
        output = std::move(instrument);
        return true;
    }
    if (type == "internal") {
        ContractReader reader(
            value, path, {"type", "id", "bypassed", "definitionJson", "definitionBaseDir", "midi"},
            error);
        juce::String actualType;
        InternalInstrumentSpec instrument;
        juce::var midi;
        if (!reader.string("type", actualType) || actualType != type ||
            !reader.string("id", instrument.id) ||
            !reader.boolean("bypassed", instrument.bypassed) ||
            !reader.string("definitionJson", instrument.definitionJson) ||
            !reader.string("definitionBaseDir", instrument.definitionBaseDir) ||
            !reader.object("midi", midi) || !reader.finish())
            return false;
        if (!decodeInstrumentMidi(midi, child(path, "midi"), instrument.midi, error)) return false;
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
    if (!std::isfinite(static_cast<float>(std::pow(10.0, output.gainDb / 20.0))))
        return fail(error, child(path, "gainDb"), "exceeds the audio engine range");
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
    if (output.type == "parameterRamp") {
        ContractReader control(
            kind, kindPath, {"type", "parameter", "fromValue", "toValue", "durationTicks"}, error);
        double endValue = 0.0;
        if (!control.string("type", type) || !control.string("parameter", output.parameter) ||
            !control.number("fromValue", number) || !control.number("toValue", endValue) ||
            !control.unsignedInteger("durationTicks", output.durationTicks) || !control.finish())
            return false;
        output.value = static_cast<float>(number);
        output.endValue = static_cast<float>(endValue);
        if (output.parameter.isEmpty() || output.durationTicks == 0 ||
            !std::isfinite(output.value) || !std::isfinite(output.endValue))
            return fail(error, kindPath, "invalid parameter ramp");
        return true;
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
        if (control.type == "parameterRamp" &&
            control.durationTicks > output.durationTicks - control.tick)
            return fail(error, child(eventPath, "durationTicks"),
                        "ramp exceeds clip durationTicks");
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
    ContractReader reader(value, path,
                          {"id", "kind", "gainDb", "pan", "muted", "solo", "armed", "monitorInput",
                           "audioInput", "midiInput", "effects", "instrument", "audioClips",
                           "midiClips", "panLaw", "externalAudioSourceTrackIds", "outputBusId"},
                          error);
    juce::String kind;
    juce::var audioInput;
    juce::var midiInput;
    juce::var instrument;
    juce::Array<juce::var> effects;
    juce::Array<juce::var> audioClips;
    juce::Array<juce::var> midiClips;
    if (!reader.string("id", output.id) || !reader.string("kind", kind) ||
        !reader.string("panLaw", output.panLaw) ||

        !reader.optionalString("outputBusId", output.outputBusId) ||
        !reader.number("gainDb", output.gainDb) || !reader.number("pan", output.pan) ||
        !reader.boolean("muted", output.muted) || !reader.boolean("solo", output.solo) ||
        !reader.boolean("armed", output.armed) ||
        !reader.boolean("monitorInput", output.monitorInput) ||
        !reader.value("audioInput", audioInput) || !reader.object("midiInput", midiInput) ||
        !reader.array("effects", effects) || !reader.value("instrument", instrument) ||
        !reader.array("audioClips", audioClips) || !reader.array("midiClips", midiClips) ||
        !reader.finish())
        return false;
    int trackKind = 0;
    if (!readEnum(kind, child(path, "kind"), {{"audio", 0}, {"instrument", 1}}, trackKind, error))
        return false;
    output.kind = static_cast<TrackKindSpec>(trackKind);
    juce::Array<juce::var> sources;
    if (!reader.array("externalAudioSourceTrackIds", sources)) return false;
    for (const auto& source : sources) {
        if (!source.isString() || source.toString().isEmpty() ||
            std::find(output.externalAudioSourceTrackIds.begin(),
                      output.externalAudioSourceTrackIds.end(),
                      source.toString()) != output.externalAudioSourceTrackIds.end())
            return fail(error, child(path, "externalAudioSourceTrackIds"),
                        "requires unique track ids");
        output.externalAudioSourceTrackIds.push_back(source.toString());
    }
    if (output.panLaw != "equalPower" && output.panLaw != "unityCenterStereo" &&
        output.panLaw != "linearBalance")
        return fail(error, child(path, "panLaw"), "unknown pan law");
    if (!std::isfinite(static_cast<float>(std::pow(10.0, output.gainDb / 20.0))))
        return fail(error, child(path, "gainDb"), "exceeds the audio engine range");
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

bool decodeEffects(const juce::Array<juce::var>& values, const juce::String& path,
                   std::vector<PluginDeviceSpec>& output, juce::String& error) {
    for (int index = 0; index < values.size(); ++index) {
        PluginDeviceSpec device;
        if (!decodePluginDevice(values[index], element(path, index), device, error)) return false;
        output.push_back(std::move(device));
    }
    return true;
}

bool decodeMixer(const juce::var& value, const juce::String& path, MixerSpec& output,
                 juce::String& error) {
    ContractReader reader(value, path, {"masterEffects", "buses", "sends", "sidechainSends"},
                          error);
    juce::Array<juce::var> effects, buses, sends, sidechainSends;
    if (!reader.array("masterEffects", effects) || !reader.array("buses", buses) ||
        !reader.array("sends", sends) || !reader.array("sidechainSends", sidechainSends) ||
        !reader.finish() ||
        !decodeEffects(effects, child(path, "masterEffects"), output.masterEffects, error))
        return false;
    for (int index = 0; index < buses.size(); ++index) {
        const auto location = element(child(path, "buses"), index);
        ContractReader bus(buses[index], location,
                           {"id", "kind", "gainDb", "pan", "panLaw", "muted", "solo", "effects"},
                           error);
        MixBusSpec spec;
        juce::String kind;
        juce::Array<juce::var> chain;
        int decodedKind = 0;
        if (!bus.string("id", spec.id) || !bus.string("kind", kind) ||
            !bus.number("gainDb", spec.gainDb) || !bus.number("pan", spec.pan) ||
            !bus.string("panLaw", spec.panLaw) || !bus.boolean("muted", spec.muted) ||
            !bus.boolean("solo", spec.solo) || !bus.array("effects", chain) || !bus.finish() ||
            !readEnum(kind, child(location, "kind"), {{"group", 0}, {"return", 1}}, decodedKind,
                      error) ||
            !decodeEffects(chain, child(location, "effects"), spec.effects, error))
            return false;
        spec.kind = static_cast<MixBusKindSpec>(decodedKind);
        if (!std::isfinite(static_cast<float>(std::pow(10.0, spec.gainDb / 20.0))) ||
            spec.pan < -1.0 || spec.pan > 1.0)
            return fail(error, location, "mix controls are outside their ranges");
        if (spec.panLaw != "equalPower" && spec.panLaw != "unityCenterStereo" &&
            spec.panLaw != "linearBalance")
            return fail(error, child(location, "panLaw"), "unknown pan law");
        output.buses.push_back(std::move(spec));
    }
    for (int index = 0; index < sends.size(); ++index) {
        const auto location = element(child(path, "sends"), index);
        ContractReader send(sends[index], location,
                            {"id", "trackId", "returnBusId", "levelDb", "position", "enabled"},
                            error);
        SendSpec spec;
        juce::String position;
        int decodedPosition = 0;
        if (!send.string("id", spec.id) || !send.string("trackId", spec.trackId) ||
            !send.string("returnBusId", spec.returnBusId) ||
            !send.number("levelDb", spec.levelDb) || !send.string("position", position) ||
            !send.boolean("enabled", spec.enabled) || !send.finish() ||
            !readEnum(position, child(location, "position"), {{"preFader", 0}, {"postFader", 1}},
                      decodedPosition, error))
            return false;
        spec.position = static_cast<SendPositionSpec>(decodedPosition);
        if (spec.levelDb < -90.0 || spec.levelDb > 24.0)
            return fail(error, child(location, "levelDb"), "must be between -90 and 24");
        output.sends.push_back(std::move(spec));
    }
    for (int index = 0; index < sidechainSends.size(); ++index) {
        const auto location = element(child(path, "sidechainSends"), index);
        ContractReader send(
            sidechainSends[index], location,
            {"id", "sourceTrackId", "targetDeviceId", "levelDb", "position", "enabled"}, error);
        SidechainSendSpec spec;
        juce::String position;
        int decodedPosition = 0;
        if (!send.string("id", spec.id) || !send.string("sourceTrackId", spec.sourceTrackId) ||
            !send.string("targetDeviceId", spec.targetDeviceId) ||
            !send.number("levelDb", spec.levelDb) || !send.string("position", position) ||
            !send.boolean("enabled", spec.enabled) || !send.finish() ||
            !readEnum(position, child(location, "position"), {{"preFader", 0}, {"postFader", 1}},
                      decodedPosition, error))
            return false;
        spec.position = static_cast<SendPositionSpec>(decodedPosition);
        if (spec.levelDb < -90.0 || spec.levelDb > 24.0)
            return fail(error, child(location, "levelDb"), "must be between -90 and 24");
        output.sidechainSends.push_back(std::move(spec));
    }
    return true;
}

bool decodeGraph(const juce::var& value, const juce::String& path, ExecutionGraph& output,
                 juce::String& error) {
    ContractReader reader(
        value, path,
        {"timebase", "loopRange", "punchRange", "metronomeEnabled", "metronomeSound",
         "masterGainDb", "tracks", "mixdown", "mixer", "automation"},
        error);
    juce::var timebase;
    juce::var loopRange;
    juce::var punchRange;
    juce::var mixdown;
    juce::var mixer;
    juce::String metronomeSound;
    juce::Array<juce::var> tracks;
    juce::Array<juce::var> automation;
    int sound = 0;
    if (!reader.object("timebase", timebase) || !reader.object("loopRange", loopRange) ||
        !reader.value("punchRange", punchRange) ||
        !reader.boolean("metronomeEnabled", output.metronomeEnabled) ||
        !reader.string("metronomeSound", metronomeSound) ||
        !reader.number("masterGainDb", output.masterGainDb) || !reader.array("tracks", tracks) ||
        !reader.object("mixdown", mixdown) || !reader.object("mixer", mixer) ||
        !reader.array("automation", automation) || !reader.finish() ||
        !readEnum(metronomeSound, child(path, "metronomeSound"), {{"beep", 0}, {"wood", 1}}, sound,
                  error) ||
        !decodeMixer(mixer, child(path, "mixer"), output.mixer, error))
        return false;
    output.metronomeSound = static_cast<MetronomeSoundSpec>(sound);
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
    for (int index = 0; index < automation.size(); ++index) {
        AutomationLaneSpec lane;
        if (!decodeAutomationLane(automation.getReference(index),
                                  element(child(path, "automation"), index), lane, error))
            return false;
        output.automation.push_back(std::move(lane));
    }
    return validateMixerGraph(output, error);
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
