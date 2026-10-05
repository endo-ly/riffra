#include <JuceHeader.h>

#include <iostream>
#include <thread>

#include "contract/ExecutionGraph.h"
#include "timeline/TimelineEngine.h"

namespace {

riffra::TimelineSnapshotSpec makeSnapshot(const juce::String& effectPath,
                                          const juce::String& instrumentPath) {
    riffra::TimelineSnapshotSpec snapshot;
    snapshot.projectId = "real-vst-runtime-test";
    snapshot.revision = 1;
    snapshot.graph.timebase = {960, 120.0, 4, 4};

    riffra::TrackSpec instrumentTrack;
    instrumentTrack.id = "track:instrument";
    instrumentTrack.kind = riffra::TrackKindSpec::instrument;
    instrumentTrack.armed = true;
    instrumentTrack.effects.push_back({"device:instrument-effect", effectPath, {}});
    instrumentTrack.instrument =
        riffra::Vst3InstrumentSpec{"device:instrument", instrumentPath, {}};
    snapshot.graph.tracks.push_back(std::move(instrumentTrack));

    riffra::TrackSpec audioTrack;
    audioTrack.id = "track:audio";
    audioTrack.kind = riffra::TrackKindSpec::audio;
    audioTrack.armed = true;
    audioTrack.monitorInput = true;
    audioTrack.effects.push_back({"device:audio-effect", effectPath, {}});
    snapshot.graph.tracks.push_back(std::move(audioTrack));
    return snapshot;
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
    if (engine.commitPreparedSnapshot(error) != riffra::RealtimeRequest::accepted) {
        std::cerr << error << '\n';
        return 1;
    }
    const auto parameters =
        engine.deviceParameterStatus("track:audio", "device:audio-effect", error);
    if (!parameters || parameters->parameters.size() < 4) {
        std::cerr << "test effect parameters missing: " << error << '\n';
        return 1;
    }
    const auto& level = parameters->parameters[1];
    const auto& mode = parameters->parameters[2];
    if (level.label != "dB" || level.displayValue.isEmpty() || level.discrete ||
        level.stepCount != 0 || !mode.discrete || mode.stepCount != 3 || mode.choices.size() != 3 ||
        mode.choices[1].displayValue != "Warm" || mode.choices[1].value != 0.5f ||
        parameters->parameters[0].defaultValue != 0.5f ||
        parameters->parameters[3].choices[0].displayValue != "Small") {
        std::cerr << "test effect metadata mismatch\n";
        return 1;
    }
    if (!engine.setDeviceParameter("track:audio", "device:audio-effect", 0, 0.25f, error)) {
        std::cerr << error << '\n';
        return 1;
    }
    const auto changed = engine.deviceParameterStatus("track:audio", "device:audio-effect", error);
    if (!changed || changed->parameters[0].value != 0.25f) return 1;
    if (!engine.setDeviceParameter("track:audio", "device:audio-effect", 2, 0.5f, error)) {
        std::cerr << error << '\n';
        return 1;
    }
    const auto modeChanged =
        engine.deviceParameterStatus("track:audio", "device:audio-effect", error);
    if (!modeChanged || modeChanged->parameters[2].displayValue != "Warm" ||
        modeChanged->parameters[3].displayValue != "Short" ||
        modeChanged->parameters[3].choices[2].displayValue != "Long") {
        std::cerr << "mode-dependent parameter metadata did not update: " << error << '\n';
        return 1;
    }
    engine.setRealtimeOwner(riffra::RealtimeOwner::audio);
    while (engine.play().has_value()) {
    }
    if (engine.setDeviceParameter("track:audio", "device:audio-effect", 2, 1.0f, error)) {
        std::cerr << "parameter command unexpectedly accepted by full queue\n";
        return 1;
    }
    engine.setRealtimeOwner(riffra::RealtimeOwner::control);
    const auto rejected = engine.deviceParameterStatus("track:audio", "device:audio-effect", error);
    if (!rejected || rejected->parameters[2].displayValue != "Warm") {
        std::cerr << "rejected parameter command changed plugin state\n";
        return 1;
    }
    engine.setRealtimeOwner(riffra::RealtimeOwner::audio);
    {
        std::jthread audio([&](std::stop_token stop) {
            juce::AudioBuffer<float> output(2, 512);
            while (!stop.stop_requested()) {
                (void)engine.beginBlock(512);
                engine.mix(output.getArrayOfWritePointers(), 2, 512);
            }
        });
        if (!engine.setDeviceParameter("track:audio", "device:audio-effect", 2, 0.0f, error)) {
            std::cerr << error << '\n';
            return 1;
        }
        const auto realtimeChanged =
            engine.deviceParameterStatus("track:audio", "device:audio-effect", error);
        if (!realtimeChanged || realtimeChanged->parameters[2].displayValue != "Clean" ||
            realtimeChanged->parameters[3].displayValue != "Small" ||
            realtimeChanged->parameters[3].choices[2].displayValue != "Large") {
            std::cerr << "realtime mode-dependent metadata did not update: " << error << '\n';
            return 1;
        }
    }
    engine.setRealtimeOwner(riffra::RealtimeOwner::control);
    return 0;
}
