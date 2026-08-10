#include "TimelineEngine.h"

#include <JuceHeader.h>

#include <iostream>

namespace {

juce::var makePluginDevice(const juce::String& id, const juce::String& path) {
    auto* device = new juce::DynamicObject();
    device->setProperty("id", id);
    device->setProperty("kind", "plugin");
    device->setProperty("path", path);
    device->setProperty("disabledPlaceholder", false);
    device->setProperty("bypassed", false);
    return juce::var(device);
}

juce::var makeTrack(
    const juce::String& id,
    const juce::String& kind,
    const juce::var& devices,
    const juce::var& instrument,
    const bool armed) {
    auto* track = new juce::DynamicObject();
    track->setProperty("id", id);
    track->setProperty("kind", kind);
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", armed);
    track->setProperty("monitoring", armed ? "on" : "off");
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", devices);
    track->setProperty("rack", juce::var(rack));
    if (!instrument.isVoid())
        track->setProperty("instrument", instrument);
    track->setProperty("audioClips", juce::Array<juce::var>());
    track->setProperty("midiClips", juce::Array<juce::var>());
    track->setProperty("automation", juce::Array<juce::var>());
    return juce::var(track);
}

juce::var makeSnapshot(const juce::String& effectPath, const juce::String& instrumentPath) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* instrument = new juce::DynamicObject();
    instrument->setProperty("id", "device:instrument");
    instrument->setProperty("kind", "plugin");
    instrument->setProperty("path", instrumentPath);
    instrument->setProperty("disabledPlaceholder", false);
    instrument->setProperty("bypassed", false);

    juce::Array<juce::var> tracks;
    juce::Array<juce::var> instrumentEffects;
    instrumentEffects.add(makePluginDevice("device:instrument-effect", effectPath));
    tracks.add(makeTrack(
        "track:instrument",
        "instrument",
        juce::var(instrumentEffects),
        juce::var(instrument),
        true));

    juce::Array<juce::var> audioEffects;
    audioEffects.add(makePluginDevice("device:audio-effect", effectPath));
    tracks.add(makeTrack(
        "track:audio",
        "audio",
        juce::var(audioEffects),
        {},
        true));

    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

}  // namespace

int main(int argc, char** argv) {
    if (argc != 3) {
        std::cerr << "expected effect and instrument VST3 paths\n";
        return 2;
    }

    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    riffra::TimelineEngine engine(true);
    juce::String error;
    const auto snapshot = makeSnapshot(argv[1], argv[2]);

    if (!engine.loadSnapshot(snapshot, formats, 48'000.0, 512, error, false)) {
        std::cerr << error << '\n';
        return 1;
    }
    if (!engine.commitPreparedSnapshot(error)) {
        std::cerr << error << '\n';
        return 1;
    }
    return 0;
}
