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
        for (;;) {
            state.wait(State::idle, std::memory_order_relaxed);
            auto expected = State::ready;
            if (state.compare_exchange_strong(expected, State::running,
                                              std::memory_order_acquire)) {
                owner.processAvailableTracks();
                state.store(State::idle, std::memory_order_release);
            } else if (expected == State::stopping) {
                return;
            }
        }
    }

    void offerWork() noexcept {
        state.store(State::ready, std::memory_order_release);
        state.notify_one();
    }

    void retractOffer() noexcept {
        auto expected = State::ready;
        state.compare_exchange_strong(expected, State::idle, std::memory_order_relaxed);
    }

    void finishStage() noexcept {
        std::size_t spins = 0;
        while (state.load(std::memory_order_acquire) == State::running) {
            if (++spins % 64 == 0) std::this_thread::yield();
        }
    }

    void requestStop() noexcept {
        state.store(State::stopping, std::memory_order_release);
        state.notify_one();
    }

private:
    enum class State { idle, ready, running, stopping };
    TrackProcessingPool& owner;
    std::atomic<State> state{State::idle};
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
        for (auto& worker : workers) worker->requestStop();
        for (auto& worker : workers) worker->waitForThreadToExit(-1);
        throw;
    }
}

TrackProcessingPool::~TrackProcessingPool() {
    for (auto& worker : workers) worker->requestStop();
    for (auto& worker : workers) worker->waitForThreadToExit(-1);
}

void TrackProcessingPool::processAvailableTracks() noexcept {
    juce::ScopedNoDenormals noDenormals;
    const auto currentTracks = tracks;
    const auto currentChannels = channels;
    const auto currentJob = job;
    const auto count = currentChannels.empty() ? currentTracks.size() : currentChannels.size();
    for (;;) {
        const auto index = nextTrack.fetch_add(1, std::memory_order_relaxed);
        if (index >= count) return;
        if (currentChannels.empty())
            processTrackStage(*currentTracks[index], currentJob);
        else {
            auto& channel = *currentChannels[index];
            const auto position = currentJob.graph->transportAt(
                currentJob.rangeStart - channel.inputLatency, currentJob.playing);
            channel.process(currentJob.sampleCount, currentJob.rangeStart, currentJob.playing,
                            &position);
        }
    }
}

void TrackProcessingPool::run(const std::span<Track* const> currentTracks,
                              const TrackStageJob& currentJob) noexcept {
    if (currentTracks.empty()) return;
    tracks = currentTracks;
    channels = {};
    job = currentJob;
    execute(currentTracks.size());
}

void TrackProcessingPool::runMixer(const std::span<MixerChannel* const> currentChannels,
                                   const int sampleCount, const std::int64_t rangeStart,
                                   const bool playing, const PreparedTimeline& graph) noexcept {
    if (currentChannels.empty()) return;
    tracks = {};
    channels = currentChannels;
    job.sampleCount = sampleCount;
    job.rangeStart = rangeStart;
    job.playing = playing;
    job.graph = &graph;
    execute(currentChannels.size());
}

void TrackProcessingPool::execute(const std::size_t count) noexcept {
    const auto workerCount = std::min(workers.size(), count - 1);
    nextTrack.store(0, std::memory_order_relaxed);
    for (std::size_t index = 0; index < workerCount; ++index) workers[index]->offerWork();
    processAvailableTracks();
    // The queue is drained. Only workers that claimed an offer may still access the stage.
    for (std::size_t index = 0; index < workerCount; ++index) workers[index]->retractOffer();
    for (std::size_t index = 0; index < workerCount; ++index) workers[index]->finishStage();
}

void processTrackStage(Track& track, const TrackStageJob& job) noexcept {
    auto& runtime = *track.runtime;
    const auto started = std::chrono::steady_clock::now();
    runtime.trackOutputBuffer.clear(0, job.sampleCount);
    runtime.preFaderOutputBuffer.clear(0, job.sampleCount);
    runtime.routedOutputBuffer.clear(0, job.sampleCount);
    if (job.kind == TrackStageKind::playback) {
        TimelineEngine::mixRange(track, job.rangeStart, 0, job.sampleCount);
        TimelineEngine::scheduleMidi(*job.graph, track, job.rangeStart, job.sampleCount);
        runtime.processedBuffer.clear(0, job.sampleCount);
        if (!runtime.instrumentTrack) {
            runtime.processTimelineInputDelay(runtime.mixBuffer, job.sampleCount,
                                              runtime.lowLatencyPdcBypassActive());
            TimelineEngine::mergeTimelineAndLiveInput(track, job.sampleCount);
        }
        if (runtime.instrumentTrack)
            TimelineEngine::processInstrumentTrack(*job.graph, track, job.sampleCount,
                                                   &runtime.midiBuffer, job.rangeStart);
        else {
            const auto position = job.graph->transportAt(
                job.rangeStart - runtime.routeLatencySamples + runtime.effectLatencySamples(),
                true);
            runtime.effects().process(
                runtime.mixBuffer.getArrayOfReadPointers(), 2,
                runtime.processedBuffer.getArrayOfWritePointers(), 2, job.sampleCount, nullptr,
                runtime.deviceAutomation,
                job.rangeStart - runtime.routeLatencySamples + runtime.effectLatencySamples(), true,
                &runtime.effectAlignment, &position);
        }
    } else if (runtime.instrumentTrack) {
        TimelineEngine::processLiveInstrumentTrack(*job.graph, track, job.sampleCount,
                                                   job.rangeStart, job.playing);
    } else {
        if (!runtime.monitorInput || runtime.audioInputChannel < 0) return;
        runtime.processedBuffer.clear(0, job.sampleCount);
        const auto position = job.graph->transportAt(job.rangeStart, false);
        runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                                  runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                  job.sampleCount, nullptr, runtime.deviceAutomation,
                                  job.rangeStart, false, &runtime.effectAlignment, &position);
    }
    TimelineEngine::mixTrackOutput(track, job.rangeStart, job.sampleCount, job.transportGainStart,
                                   job.transportGainStep, job.playing);
    const auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
                             std::chrono::steady_clock::now() - started)
                             .count();
    runtime.windowProcessingTotalUs += static_cast<std::uint64_t>(elapsed);
    runtime.windowProcessingMaximumUs =
        std::max(runtime.windowProcessingMaximumUs, static_cast<std::uint64_t>(elapsed));
    ++runtime.windowProcessingCount;
}

}  // namespace riffra
