#include "TrackProcessingPool.h"

#include <algorithm>
#include <stdexcept>
#include <thread>

#include "TimelineEngine.h"

namespace riffra {

class TrackProcessingPool::Worker final : public juce::Thread {
public:
    explicit Worker(TrackProcessingPool& pool) : Thread("Riffra track processing"), owner(pool) {}

    void run() override {
        std::uint64_t lastGeneration = 0;
        for (;;) {
            owner.wake.acquire();
            if (owner.stopping.load(std::memory_order_acquire)) return;
            const auto current = owner.generation.load(std::memory_order_acquire);
            if (current != lastGeneration) {
                juce::ScopedNoDenormals noDenormals;
                owner.processAvailableTracks();
                lastGeneration = current;
            }
            // Each wake token is acknowledged, even if this worker already
            // consumed another token for the same generation. No job state is
            // accessed after acknowledgement.
            owner.pendingWorkers.fetch_sub(1, std::memory_order_acq_rel);
        }
    }

private:
    TrackProcessingPool& owner;
};

TrackProcessingPool::TrackProcessingPool(const int workerCount) {
    workers.reserve(static_cast<std::size_t>(std::clamp(workerCount, 0, 7)));
    try {
        for (int index = 0; index < std::clamp(workerCount, 0, 7); ++index) {
            auto worker = std::make_unique<Worker>(*this);
            if (!worker->startRealtimeThread(juce::Thread::RealtimeOptions{}))
                throw std::runtime_error("could not start realtime track worker");
            workers.push_back(std::move(worker));
        }
    } catch (...) {
        stopping.store(true, std::memory_order_release);
        wake.release(static_cast<int>(workers.size()));
        for (auto& worker : workers) worker->waitForThreadToExit(-1);
        throw;
    }
}

TrackProcessingPool::~TrackProcessingPool() {
    stopping.store(true, std::memory_order_release);
    wake.release(static_cast<int>(workers.size()));
    for (auto& worker : workers) worker->waitForThreadToExit(-1);
}

void TrackProcessingPool::processAvailableTracks() noexcept {
    const auto currentTracks = tracks;
    const auto currentJob = job;
    for (;;) {
        const auto index = nextTrack.fetch_add(1, std::memory_order_relaxed);
        if (index >= currentTracks.size()) return;
        processTrackStage(*currentTracks[index], currentJob);
        completedTracks.fetch_add(1, std::memory_order_acq_rel);
    }
}

void TrackProcessingPool::run(const std::span<Track* const> currentTracks,
                              const TrackStageJob& currentJob) noexcept {
    if (currentTracks.empty()) return;
    tracks = currentTracks;
    job = currentJob;
    nextTrack.store(0, std::memory_order_relaxed);
    completedTracks.store(0, std::memory_order_relaxed);
    pendingWorkers.store(static_cast<int>(workers.size()), std::memory_order_relaxed);
    generation.fetch_add(1, std::memory_order_release);
    wake.release(static_cast<int>(workers.size()));
    {
        juce::ScopedNoDenormals noDenormals;
        processAvailableTracks();
    }
    std::size_t spins = 0;
    while (completedTracks.load(std::memory_order_acquire) != currentTracks.size() ||
           pendingWorkers.load(std::memory_order_acquire) != 0) {
        if (++spins % 64 == 0) std::this_thread::yield();
    }
}

void processTrackStage(Track& track, const TrackStageJob& job) noexcept {
    auto& runtime = *track.runtime;
    if (job.kind == TrackStageKind::liveInstrument && !runtime.instrumentTrack) return;
    if (job.kind == TrackStageKind::liveAudioMonitor && runtime.instrumentTrack) return;
    const auto started = std::chrono::steady_clock::now();
    runtime.trackOutputBuffer.clear(0, job.sampleCount);
    if (job.kind == TrackStageKind::playback) {
        TimelineEngine::mixRange(track, job.rangeStart, 0, job.sampleCount);
        TimelineEngine::scheduleMidi(*job.graph, track, job.rangeStart, job.sampleCount);
        runtime.processedBuffer.clear(0, job.sampleCount);
        if (!runtime.instrumentTrack)
            TimelineEngine::mergeTimelineAndLiveInput(track, job.sampleCount);
        if (runtime.instrumentTrack)
            TimelineEngine::processInstrumentTrack(*job.graph, track, job.sampleCount,
                                                   &runtime.midiBuffer, job.rangeStart);
        else
            runtime.effects().process(runtime.mixBuffer.getArrayOfReadPointers(), 2,
                                      runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                      job.sampleCount);
    } else if (job.kind == TrackStageKind::liveInstrument) {
        TimelineEngine::processLiveInstrumentTrack(*job.graph, track, job.sampleCount,
                                                   job.rangeStart, job.playing);
    } else {
        if (!runtime.monitorInput || runtime.audioInputChannel < 0) return;
        runtime.processedBuffer.clear(0, job.sampleCount);
        runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                                  runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                  job.sampleCount);
    }
    const auto audible = !runtime.muted && (!job.graph->hasSolo || runtime.solo);
    TimelineEngine::mixTrackOutput(track, audible, job.rangeStart, job.sampleCount,
                                   job.transportGainStart, job.transportGainStep);
    const auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
                             std::chrono::steady_clock::now() - started)
                             .count();
    runtime.windowProcessingTotalUs += static_cast<std::uint64_t>(elapsed);
    runtime.windowProcessingMaximumUs =
        std::max(runtime.windowProcessingMaximumUs, static_cast<std::uint64_t>(elapsed));
    ++runtime.windowProcessingCount;
}

}  // namespace riffra
