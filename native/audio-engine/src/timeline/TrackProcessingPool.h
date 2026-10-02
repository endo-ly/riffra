#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <semaphore>
#include <span>
#include <vector>

#include "PreparedTimeline.h"

namespace riffra {

using Track = PreparedTimeline::Track;

enum class TrackStageKind { playback, liveInstrument, liveAudioMonitor };

struct TrackStageJob final {
    TrackStageKind kind;
    const PreparedTimeline* graph;
    std::int64_t rangeStart;
    int destinationStart;
    int sampleCount;
    float transportGainStart;
    float transportGainStep;
    bool playing;
};

void processTrackStage(Track& track, const TrackStageJob& job) noexcept;

/// Runs independent track DSP and joins it before the caller mixes the master.
class TrackProcessingPool final {
public:
    explicit TrackProcessingPool(int workerCount);
    ~TrackProcessingPool();
    void run(std::span<Track* const> tracks, const TrackStageJob& job) noexcept;

private:
    class Worker;
    void processAvailableTracks() noexcept;

    std::vector<std::unique_ptr<Worker>> workers;
    std::counting_semaphore<7> wake{0};
    std::atomic<bool> stopping{false};
    std::atomic<std::uint64_t> generation{0};
    std::atomic<std::size_t> nextTrack{0};
    std::atomic<std::size_t> completedTracks{0};
    std::atomic<int> pendingWorkers{0};
    std::span<Track* const> tracks;
    TrackStageJob job{};
};

}  // namespace riffra
