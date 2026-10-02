#pragma once

#include <JuceHeader.h>

#include <array>
#include <cstdint>
#include <memory>
#include <optional>
#include <vector>

#include "TimelineTimebase.h"
#include "TrackRuntime.h"
#include "contract/ExecutionGraph.h"

namespace riffra {

/// Values of a prepared graph that never change after it is built.
struct GraphSummary final {
    std::uint64_t trackCount = 0;
    std::uint64_t instrumentRuntimeCount = 0;
    std::uint64_t pluginCount = 0;
    std::uint64_t maximumLatencySamples = 0;
    std::vector<juce::String> armedTrackIds;
    /// Whether any Audio Track monitors its live input.
    bool monitorLiveInput = false;
    /// Physical input channels routed to monitored Audio Tracks, as a bit set.
    std::uint32_t monitoringInputChannels = 0;
    /// Whether an armed Instrument Track receives live MIDI input.
    bool armedInstrumentTrack = false;
};

/// A fully prepared Arrangement Graph.
///
/// Built on the message thread, published to the audio thread by one realtime
/// command, and destroyed on the message thread after the audio thread retires
/// it. Only per-track runtime state changes after publication.
struct PreparedTimeline final {
    struct Clip final {
        juce::String id;
        std::unique_ptr<juce::AudioFormatReaderSource> readerSource;
        std::unique_ptr<juce::BufferingAudioSource> bufferingSource;
        juce::PositionableAudioSource* positionableSource = nullptr;
        std::unique_ptr<juce::ResamplingAudioSource> resamplingSource;
        juce::AudioBuffer<float> scratch;
        std::int64_t startSample = 0;
        std::int64_t sourceStartFrame = 0;
        std::int64_t sourceEndFrame = 0;
        std::int64_t durationSamples = 0;
        std::int64_t expectedSourceFrame = -1;
        double sourceSampleRate = 0.0;
        float gain = 1.0f;
        float pan = 0.0f;
        float leftGain = 1.0f;
        float rightGain = 1.0f;
        std::int64_t fadeInSamples = 0;
        std::int64_t fadeOutSamples = 0;
        int fadeShape = 1;
        bool loop = false;
        bool muted = false;
        ProcessingStage processingStage = ProcessingStage::PreEffects;
    };

    struct Track final {
        juce::String id;
        std::vector<std::unique_ptr<Clip>> clips;
        std::unique_ptr<TrackRuntime> runtime;
        juce::String instrumentDeviceId;
        std::vector<PluginDeviceSpec> effects;
        std::optional<InstrumentSpec> instrument;
        // Device instances are shared with the committed graph only when both
        // topology and persisted state match it. A state change receives newly
        // prepared plugin instances so state application never mutates a
        // published graph.
        bool reuseRuntimeDevices = false;
    };

    /// Assigned by the registry when the graph is committed.
    std::uint64_t serial = 0;
    juce::String projectId;
    std::uint64_t meterEpoch = 0;
    std::uint64_t revision = 0;
    TimelineTimebase timebase;
    double outputSampleRate = 0.0;
    int preparedBlockSize = 512;
    bool loopEnabled = false;
    std::int64_t loopStartSample = 0;
    std::int64_t loopEndSample = 0;
    bool punchEnabled = false;
    std::int64_t punchStartSample = 0;
    std::int64_t punchEndSample = 0;
    bool metronomeEnabled = false;
    float masterGainDb = 0.0f;
    bool hasSolo = false;
    std::int64_t beatSamples = 0;
    std::int64_t beatsPerBar = 4;
    std::uint16_t timeSignatureNumerator = 4;
    std::uint16_t timeSignatureDenominator = 4;
    GraphSummary summary;
    std::vector<std::unique_ptr<Track>> tracks;
    std::vector<Track*> processingTracks;

    [[nodiscard]] Track* findTrack(const juce::String& trackId) const noexcept {
        for (const auto& track : tracks)
            if (track->id == trackId) return track.get();
        return nullptr;
    }
};

}  // namespace riffra
