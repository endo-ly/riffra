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

bool TimelineEngine::loadSnapshot(const juce::var& snapshot, juce::AudioFormatManager& formats,
                                  const double outputSampleRate, const int maximumBlockSize,
                                  juce::String& error, const bool commitImmediately) {
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
        if (timeline != nullptr) {
            // Validate every reusable runtime before moving ownership. The
            // prepared graph was built against the active graph, but a direct
            // native mutation may have changed the topology in the meantime.
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices) continue;
                const auto existing =
                    std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                 [&candidateTrack](const auto& item) {
                                     return item->id == candidateTrack->id &&
                                            item->effectTopologySignature ==
                                                candidateTrack->effectTopologySignature &&
                                            item->instrumentTopologySignature ==
                                                candidateTrack->instrumentTopologySignature;
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
                const auto existing =
                    std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                 [&candidateTrack](const auto& item) {
                                     return item->id == candidateTrack->id &&
                                            item->effectTopologySignature ==
                                                candidateTrack->effectTopologySignature &&
                                            item->instrumentTopologySignature ==
                                                candidateTrack->instrumentTopologySignature;
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
    }
    reclaimRetiredTimelines();
    return true;
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    pendingTimeline.reset();
}

void TimelineEngine::startPreparing() noexcept {
    state.store(State::starting, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::play() noexcept {
    state.store(State::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(State::stopped, std::memory_order_release);
    resetPlaybackPending.store(true, std::memory_order_release);
    requestPlaybackReset();
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
    const auto currentState = state.load(std::memory_order_acquire);
    switch (currentState) {
        case State::playing:
            // Playback seeks cross the audio callback boundary so the existing
            // discontinuity handling remains atomic with the new position.
            pendingSeekSample.store(sample, std::memory_order_release);
            seekPending.store(true, std::memory_order_release);
            requestPlaybackReset();
            discontinuity.fetch_add(1, std::memory_order_relaxed);
            break;
        case State::stopped:
        case State::starting:
        case State::faulted:
            // Starting has not crossed into playback yet, and faulted cannot
            // render playback. Both use the stopped cursor semantics.
            timelineSample.store(sample, std::memory_order_release);
            seekPending.store(false, std::memory_order_release);
            break;
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
}

juce::var TimelineEngine::status() const {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "transportStatus");
    const auto currentState = state.load(std::memory_order_acquire);
    object->setProperty("state", currentState == State::playing    ? "playing"
                                 : currentState == State::starting ? "starting"
                                 : currentState == State::faulted  ? "faulted"
                                                                   : "stopped");
    object->setProperty("timelineSample",
                        static_cast<juce::int64>(timelineSample.load(std::memory_order_acquire)));
    object->setProperty("audioClockSample",
                        static_cast<juce::int64>(audioClockSample.load(std::memory_order_acquire)));
    object->setProperty(
        "sequence", static_cast<juce::int64>(sequence.fetch_add(1, std::memory_order_relaxed) + 1));
    object->setProperty("graphRevision", 0);
    object->setProperty(
        "graphPublishCount",
        static_cast<juce::int64>(graphPublishCount.load(std::memory_order_acquire)));
    object->setProperty("trackCount", 0);
    object->setProperty("instrumentRuntimeCount", 0);
    object->setProperty("pluginCount", 0);
    object->setProperty("maximumLatencySamples", 0);
    object->setProperty("liveMidiDrops", 0);
    object->setProperty("clockGeneration",
                        static_cast<juce::int64>(clockGeneration.load(std::memory_order_acquire)));
    object->setProperty("discontinuity",
                        static_cast<juce::int64>(discontinuity.load(std::memory_order_acquire)));
    object->setProperty("revision", 0);
    object->setProperty("sampleRate", 0.0);
    object->setProperty("timelineTick", 0);
    const auto phase = recordingPhase.load(std::memory_order_acquire);
    object->setProperty("recordingPhase", phase == RecordingPhase::countingIn  ? "countingIn"
                                          : phase == RecordingPhase::recording ? "recording"
                                          : phase == RecordingPhase::stopping  ? "stopping"
                                                                               : "idle");
    object->setProperty(
        "recordingStartTick",
        static_cast<juce::int64>(recordingStartTick.load(std::memory_order_acquire)));
    object->setProperty("recordingPassOrdinal",
                        static_cast<int>(recordingPassOrdinal.load(std::memory_order_acquire)));
    object->setProperty("recordingCaptureErrors",
                        static_cast<juce::int64>(recordingCapture->captureErrors()));
    object->setProperty("unavailableClipIds", juce::Array<juce::var>{});
    object->setProperty("missingDeviceIds", juce::Array<juce::var>{});
    object->setProperty("instrumentFaults", juce::Array<juce::var>{});
    juce::Array<juce::var> armedTrackIds;
    juce::Array<juce::var> instrumentFaults;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("graphRevision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        std::uint64_t liveMidiDrops = 0;
        int instrumentRuntimeCount = 0;
        int pluginCount = 0;
        int maximumLatencySamples = 0;
        const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate));
        object->setProperty("timelineTick", tick);
        object->setProperty("recordingCurrentTick", tick);
        object->setProperty("unavailableClipIds", timeline->unavailableClipIds);
        object->setProperty("missingDeviceIds", timeline->missingDeviceIds);
        for (const auto& track : timeline->tracks) {
            if (track == nullptr) continue;
            if (track->runtime != nullptr) {
                if (track->runtime->instrument() != nullptr) ++instrumentRuntimeCount;
                pluginCount += track->runtime->effects().size();
                maximumLatencySamples =
                    std::max(maximumLatencySamples, track->runtime->pluginLatencySamples());
                if (track->runtime->instrument() != nullptr)
                    liveMidiDrops += track->runtime->instrument()->droppedMidiEvents();
            }
            if (track->runtime != nullptr && track->runtime->armed) armedTrackIds.add(track->id);
            if (track->runtime == nullptr || track->runtime->instrument() == nullptr) continue;
            auto* fault = new juce::DynamicObject();
            fault->setProperty("trackId", track->id);
            fault->setProperty("instrumentType", track->runtime->instrument()->typeName());
            fault->setProperty("faultCode",
                               static_cast<juce::int64>(track->runtime->instrument()->faultCode()));
            const auto droppedMidi = track->runtime->instrument()->droppedMidiEvents();
            fault->setProperty("droppedMidiEvents", static_cast<juce::int64>(droppedMidi));
            instrumentFaults.add(juce::var(fault));
        }
        object->setProperty("trackCount", static_cast<int>(timeline->tracks.size()));
        object->setProperty("instrumentRuntimeCount", instrumentRuntimeCount);
        object->setProperty("pluginCount", pluginCount);
        object->setProperty("maximumLatencySamples", maximumLatencySamples);
        object->setProperty("liveMidiDrops", static_cast<juce::int64>(liveMidiDrops));
    }
    object->setProperty("armedTrackIds", armedTrackIds);
    object->setProperty("instrumentFaults", instrumentFaults);
    return juce::var(object);
}

}  // namespace riffra
