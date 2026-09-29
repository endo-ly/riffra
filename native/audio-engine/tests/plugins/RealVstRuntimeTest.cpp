#include <JuceHeader.h>

#include <iostream>

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
    instrumentTrack.monitoring = riffra::MonitoringSpec::on;
    instrumentTrack.effects.push_back({"device:instrument-effect", effectPath, {}});
    instrumentTrack.instrument =
        riffra::Vst3InstrumentSpec{"device:instrument", instrumentPath, {}};
    snapshot.graph.tracks.push_back(std::move(instrumentTrack));

    riffra::TrackSpec audioTrack;
    audioTrack.id = "track:audio";
    audioTrack.kind = riffra::TrackKindSpec::audio;
    audioTrack.armed = true;
    audioTrack.monitoring = riffra::MonitoringSpec::on;
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
    return 0;
}
