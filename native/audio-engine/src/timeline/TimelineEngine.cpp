#include "TimelineEngine.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <limits>
#include <thread>
#include <utility>

#include "ArrangementGraph.h"
#include "TimelineSnapshotBuilder.h"

namespace riffra {

float fadeEnvelope(const float progress, const int fadeShape) noexcept {
    switch (fadeShape) {
        case 0:
            return progress;
        case 2:
            return progress * progress * (3.0f - 2.0f * progress);
        default:
            return std::sin(juce::MathConstants<float>::halfPi * progress);
    }
}

bool TimelineEngine::beginAudioRead(PreparedTimeline*& active) noexcept {
    // Enter the reader section before loading the pointer. A publisher swaps
    // the pointer first and only reclaims retired graphs after this counter
    // reaches zero, so a callback always observes either the old or new graph.
    activeAudioReaders.fetch_add(1, std::memory_order_acq_rel);
    active = activeTimeline.load(std::memory_order_acquire);
    return true;
}

void TimelineEngine::endAudioRead() noexcept {
    activeAudioReaders.fetch_sub(1, std::memory_order_release);
}

bool TimelineEngine::waitForAudioReaders(const std::chrono::milliseconds timeout) noexcept {
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    while (activeAudioReaders.load(std::memory_order_acquire) != 0) {
        if (std::chrono::steady_clock::now() >= deadline) return false;
        std::this_thread::yield();
    }
    return true;
}

void TimelineEngine::reclaimRetiredTimelines() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (activeAudioReaders.load(std::memory_order_acquire) != 0) return;
    retiredTimelines.clear();
}

void TimelineEngine::serviceDeferredCleanup() noexcept { reclaimRetiredTimelines(); }

void TimelineEngine::setProjectBoundaryCallback(std::function<void(std::uint64_t)> callback) {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    projectBoundaryCallback = std::move(callback);
}

bool TimelineEngine::setTrackMixControl(const juce::String& trackId,
                                        const std::optional<float> gainDb,
                                        const std::optional<float> pan,
                                        juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A track id is required.";
        return false;
    }
    if (!gainDb.has_value() && !pan.has_value()) {
        error = "At least one of gainDb or pan is required.";
        return false;
    }
    if ((gainDb.has_value() && !std::isfinite(*gainDb)) ||
        (pan.has_value() && !std::isfinite(*pan))) {
        error = "Track mix values must be finite.";
        return false;
    }

    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "The active Timeline graph is unavailable.";
        return false;
    }
    const auto match = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&trackId](const auto& track) { return track != nullptr && track->id == trackId; });
    if (match == timeline->tracks.end() || *match == nullptr) {
        error = "The requested Track is not present in the active Timeline graph.";
        return false;
    }

    auto& runtime = *(*match)->runtime;
    if (gainDb.has_value())
        runtime.gainDb.store(juce::jlimit(-90.0f, 24.0f, *gainDb), std::memory_order_release);
    if (pan.has_value())
        runtime.pan.store(juce::jlimit(-1.0f, 1.0f, *pan), std::memory_order_release);
    return true;
}

std::vector<TrackMeterSpec> TimelineEngine::meterSnapshot() {
    std::vector<TrackMeterSpec> meters;
    AudioReadScope read(*this);
    const auto* active = read.get();
    if (active == nullptr) return meters;
    meters.reserve(active->tracks.size());
    for (const auto& track : active->tracks) {
        if (track == nullptr || track->runtime == nullptr) continue;
        const auto snapshot = track->runtime->meter.consume();
        meters.push_back({track->id, snapshot.peakLeft, snapshot.peakRight, snapshot.rmsLeft,
                          snapshot.rmsRight});
    }
    return meters;
}

juce::String TimelineEngine::activeProjectId() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return timeline != nullptr ? timeline->projectId : juce::String{};
}

float TimelineEngine::activeMasterGainDb() const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return timeline != nullptr ? timeline->masterGainDb : 0.0f;
}

TimelineEngine::ActiveProjectMeterIdentity TimelineEngine::activeProjectMeterIdentity() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return {};
    return {timeline->projectId, timeline->meterEpoch};
}

TimelineEngine::TimelineEngine(const bool offline)
    : offlineMode(offline), recordingCapture(std::make_unique<RecordingCaptureRuntime>()) {
    if (!offlineMode) readAheadThread.startThread();
}

TimelineEngine::~TimelineEngine() {
    stop();
    if (!waitForAudioReaders(std::chrono::milliseconds(250))) std::_Exit(125);
    activeTimeline.store(nullptr, std::memory_order_release);
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
        pendingTimeline.reset();
        retiredTimelines.clear();
    }
    if (readAheadThread.isThreadRunning()) readAheadThread.stopThread(3000);
}

InstrumentProcessContext TimelineEngine::instrumentProcessContext(const PreparedTimeline& prepared,
                                                                  const std::int64_t rangeStart,
                                                                  const bool playing) noexcept {
    const auto sample = std::max<std::int64_t>(0, rangeStart);
    const auto tick = prepared.timebase.sampleToTick(sample, prepared.outputSampleRate);
    const auto beatPosition =
        static_cast<double>(tick) / static_cast<double>(prepared.timebase.ppq);
    const auto beatsPerBar = static_cast<double>(prepared.timeSignatureNumerator) * 4.0 /
                             static_cast<double>(prepared.timeSignatureDenominator);
    return {
        static_cast<std::uint64_t>(sample),
        prepared.timebase.bpm,
        beatPosition,
        beatsPerBar > 0.0 ? beatPosition / beatsPerBar : 0.0,
        prepared.timeSignatureNumerator,
        prepared.timeSignatureDenominator,
        playing,
    };
}

bool TimelineEngine::loadSnapshot(const TimelineSnapshotSpec& snapshot,
                                  juce::AudioFormatManager& formats, const double outputSampleRate,
                                  const int maximumBlockSize, juce::String& error,
                                  const bool commitImmediately) {
    std::unique_ptr<PreparedTimeline> prepared;
    bool monitorLiveInputState = false;
    std::uint32_t monitoringInputChannelsState = 0;
    bool armedInstrumentTrackState = false;
    TimelineSnapshotBuilder builder(*this);
    if (!builder.build(snapshot, formats, outputSampleRate, maximumBlockSize, prepared,
                       monitorLiveInputState, monitoringInputChannelsState,
                       armedInstrumentTrackState, error))
        return false;

    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        pendingTimeline = std::move(prepared);
        pendingMonitorLiveInput = monitorLiveInputState;
        pendingMonitoringInputChannels = monitoringInputChannelsState;
        pendingArmedInstrumentTrack = armedInstrumentTrackState;
    }
    if (!commitImmediately) return true;
    return commitPreparedSnapshot(error);
}

bool TimelineEngine::commitPreparedSnapshot(juce::String& error) noexcept {
    std::unique_ptr<PreparedTimeline> candidate;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        const auto hadActiveTimeline = timeline != nullptr;
        if (pendingTimeline == nullptr) {
            error = "No prepared Timeline snapshot is available.";
            return false;
        }

        candidate = std::move(pendingTimeline);
        const auto crossesProjectBoundary =
            timeline == nullptr || timeline->projectId != candidate->projectId;
        candidate->meterEpoch = crossesProjectBoundary
                                    ? projectMeterEpoch.fetch_add(1, std::memory_order_relaxed) + 1
                                    : timeline->meterEpoch;
        if (timeline != nullptr) {
            // Validate every reusable runtime before moving ownership. The
            // prepared graph was built against the active graph, but a direct
            // native mutation may have changed the topology in the meantime.
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices) continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id &&
                               sameEffectTopology(item->effects, candidateTrack->effects) &&
                               sameInstrumentTopology(item->instrument, candidateTrack->instrument);
                    });
                if (existing == timeline->tracks.end()) {
                    error = "Timeline device runtime changed while the snapshot was prepared.";
                    pendingTimeline = std::move(candidate);
                    return false;
                }
                if (!(*existing)->runtime->prepareTimelineMidiCapacity(
                        candidateTrack->runtime->midiEventCapacity, error)) {
                    pendingTimeline = std::move(candidate);
                    return false;
                }
            }
            // State application already happened while the candidate was
            // prepared. Publishing now only transfers reusable ownership and
            // swaps the graph pointer; no VST lifecycle method runs under this
            // lock.
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices) continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id &&
                               sameEffectTopology(item->effects, candidateTrack->effects) &&
                               sameInstrumentTopology(item->instrument, candidateTrack->instrument);
                    });
                if (existing == timeline->tracks.end()) continue;
                // The candidate owns the new canonical Track state. Transfer
                // only the already-live device instances; moving the whole
                // TrackRuntime would discard that prepared state.
                candidateTrack->runtime->replaceDeviceRuntimeFrom(*(*existing)->runtime);
            }
        }

        if (timeline != nullptr) retiredTimelines.push_back(std::move(timeline));
        timeline = std::move(candidate);
        activeTimeline.store(timeline.get(), std::memory_order_release);
        runtimeDevicesNeedReprepare.store(false, std::memory_order_release);
        monitorLiveInput.store(pendingMonitorLiveInput, std::memory_order_release);
        monitoringInputChannels.store(pendingMonitoringInputChannels, std::memory_order_release);
        armedInstrumentTrack.store(pendingArmedInstrumentTrack, std::memory_order_release);
        if (!hadActiveTimeline) timelineSample.store(0, std::memory_order_release);
        discontinuity.fetch_add(1, std::memory_order_relaxed);
        graphPublishCount.fetch_add(1, std::memory_order_relaxed);
        sequence.fetch_add(1, std::memory_order_relaxed);
        if (crossesProjectBoundary && projectBoundaryCallback != nullptr)
            projectBoundaryCallback(timeline->meterEpoch);
    }
    reclaimRetiredTimelines();
    return true;
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    pendingTimeline.reset();
}

void TimelineEngine::startPreparing() noexcept {
    state.store(TransportState::starting, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::play() noexcept {
    state.store(TransportState::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(TransportState::stopped, std::memory_order_release);
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    // Keep the previous graph published while the new device environment is
    // being prepared. AudioRenderPipeline owns the mute during this period;
    // there is never a null active graph between device start and projection.
    resetPlaybackPending.store(true, std::memory_order_release);
    requestPlaybackReset();
    runtimeDevicesNeedReprepare.store(true, std::memory_order_release);
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return;
    const auto sample = timeline->timebase.tickToSample(tick, timeline->outputSampleRate);
    pendingSeekSample.store(sample, std::memory_order_release);
    seekPending.store(true, std::memory_order_release);
    seekRequestedWhileStopped.store(
        state.load(std::memory_order_acquire) != TransportState::playing,
        std::memory_order_release);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

TimelineStatus TimelineEngine::status() const {
    TimelineStatus status;
    status.transportState = state.load(std::memory_order_acquire);
    status.recordingPhase = recordingPhase.load(std::memory_order_acquire);
    status.timelineSample = seekPending.load(std::memory_order_acquire)
                                ? pendingSeekSample.load(std::memory_order_acquire)
                                : timelineSample.load(std::memory_order_acquire);
    status.audioClockSample = audioClockSample.load(std::memory_order_acquire);
    status.sequence = sequence.fetch_add(1, std::memory_order_relaxed) + 1;
    status.recordingStartTick = recordingStartTick.load(std::memory_order_acquire);
    status.recordingPassOrdinal = recordingPassOrdinal.load(std::memory_order_acquire);
    status.clockGeneration = clockGeneration.load(std::memory_order_acquire);
    status.discontinuity = discontinuity.load(std::memory_order_acquire);
    status.graphPublishCount = graphPublishCount.load(std::memory_order_acquire);
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return status;
    auto& graph = status.graph.emplace();
    graph.revision = timeline->revision;
    graph.sampleRate = timeline->outputSampleRate;
    graph.timelineTick = timeline->timebase.sampleToTick(
        timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate);
    graph.trackCount = timeline->tracks.size();
    for (const auto& track : timeline->tracks) {
        if (track == nullptr || track->runtime == nullptr) continue;
        const auto& runtime = *track->runtime;
        graph.pluginCount += static_cast<std::uint64_t>(runtime.effects().size());
        graph.maximumLatencySamples =
            std::max<std::uint64_t>(graph.maximumLatencySamples,
                                    static_cast<std::uint64_t>(runtime.pluginLatencySamples()));
        if (runtime.armed) graph.armedTrackIds.push_back(track->id);
        const auto* instrument = runtime.instrument();
        if (instrument == nullptr) continue;
        ++graph.instrumentRuntimeCount;
        graph.liveMidiDrops += instrument->droppedMidiEvents();
        graph.instrumentFaults.push_back({track->id, instrument->typeName(),
                                          instrument->faultCode(),
                                          instrument->droppedMidiEvents()});
    }
    return status;
}

}  // namespace riffra
