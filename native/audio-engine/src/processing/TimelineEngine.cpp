#include "TimelineEngine.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <limits>
#include <thread>

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

class TimelineEngine::AudioReadScope final {
public:
    explicit AudioReadScope(TimelineEngine& owner) : engine(owner) {
        entered = engine.beginAudioRead(active);
    }

    ~AudioReadScope() {
        if (entered) engine.endAudioRead();
    }

    [[nodiscard]] PreparedTimeline* get() const noexcept { return active; }
    [[nodiscard]] bool enteredSuccessfully() const noexcept { return entered && active != nullptr; }

private:
    TimelineEngine& engine;
    PreparedTimeline* active = nullptr;
    bool entered = false;
};

class TimelineEngine::AudioPublishScope final {
public:
    explicit AudioPublishScope(TimelineEngine& owner) : engine(owner) {
        engine.publishInProgress.store(true, std::memory_order_release);
        ready = engine.waitForAudioReaders(std::chrono::milliseconds(100));
    }

    ~AudioPublishScope() { engine.publishInProgress.store(false, std::memory_order_release); }

    [[nodiscard]] bool isReady() const noexcept { return ready; }

private:
    TimelineEngine& engine;
    bool ready = false;
};

bool TimelineEngine::beginAudioRead(PreparedTimeline*& active) noexcept {
    active = nullptr;
    if (publishInProgress.load(std::memory_order_acquire)) {
        callbackPublishMisses.fetch_add(1, std::memory_order_relaxed);
        return false;
    }
    activeAudioReaders.fetch_add(1, std::memory_order_acq_rel);
    if (publishInProgress.load(std::memory_order_acquire)) {
        activeAudioReaders.fetch_sub(1, std::memory_order_acq_rel);
        callbackPublishMisses.fetch_add(1, std::memory_order_relaxed);
        return false;
    }
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

TimelineEngine::TimelineEngine(const bool offline)
    : offlineMode(offline), recordingCapture(std::make_unique<RecordingCaptureRuntime>()) {
    if (!offlineMode) readAheadThread.startThread();
}

TimelineEngine::~TimelineEngine() {
    stop();
    publishInProgress.store(true, std::memory_order_release);
    if (!waitForAudioReaders(std::chrono::milliseconds(250))) std::_Exit(125);
    activeTimeline.store(nullptr, std::memory_order_release);
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
        pendingTimeline.reset();
    }
    publishInProgress.store(false, std::memory_order_release);
    if (readAheadThread.isThreadRunning()) readAheadThread.stopThread(3000);
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
    const AudioPublishScope publish(*this);
    if (!publish.isReady()) {
        error = "Native audio did not acknowledge the graph publish within 100 milliseconds.";
        return false;
    }

    std::unique_ptr<PreparedTimeline> candidate;
    std::unique_ptr<PreparedTimeline> retiredTimeline;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
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
                // The candidate deliberately skipped construction for a
                // reusable topology. Transfer the already-live instances
                // into the candidate graph; assigning the empty candidate
                // chains back to the old graph would destroy the runtime we
                // intended to reuse.
                candidateTrack->effectChain = std::move((*existing)->effectChain);
                candidateTrack->liveEffectChain = std::move((*existing)->liveEffectChain);
                candidateTrack->recordingCapture.effectChain =
                    std::move((*existing)->recordingCapture.effectChain);
                candidateTrack->instrumentRack = std::move((*existing)->instrumentRack);
                candidateTrack->liveInstrumentRack = std::move((*existing)->liveInstrumentRack);
            }
        }

        retiredTimeline = std::move(timeline);
        timeline = std::move(candidate);
        activeTimeline.store(timeline.get(), std::memory_order_release);
        runtimeDevicesNeedReprepare.store(false, std::memory_order_release);
        monitorLiveInput.store(pendingMonitorLiveInput, std::memory_order_release);
        monitoringInputChannels.store(pendingMonitoringInputChannels, std::memory_order_release);
        armedInstrumentTrack.store(pendingArmedInstrumentTrack, std::memory_order_release);
        if (retiredTimeline == nullptr) timelineSample.store(0, std::memory_order_release);
        discontinuity.fetch_add(1, std::memory_order_relaxed);
        sequence.fetch_add(1, std::memory_order_relaxed);
    }
    retiredTimeline.reset();
    return true;
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    pendingTimeline.reset();
}

void TimelineEngine::play() noexcept {
    state.store(State::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(State::stopped, std::memory_order_release);
    const AudioPublishScope publish(*this);
    if (publish.isReady()) {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        if (timeline != nullptr) {
            resetPlaybackTrackState(*timeline);
            resetRecordingTrackState(*timeline);
        }
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    const AudioPublishScope publish(*this);
    if (publish.isReady()) {
        activeTimeline.store(nullptr, std::memory_order_release);
        monitorLiveInput.store(false, std::memory_order_release);
        monitoringInputChannels.store(0, std::memory_order_release);
        armedInstrumentTrack.store(false, std::memory_order_release);
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        if (timeline != nullptr) {
            resetPlaybackTrackState(*timeline);
            resetRecordingTrackState(*timeline);
        }
    }
    runtimeDevicesNeedReprepare.store(true, std::memory_order_release);
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const AudioPublishScope publish(*this);
    if (!publish.isReady()) return;
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return;
    timelineSample.store(timeline->timebase.tickToSample(tick, timeline->outputSampleRate),
                         std::memory_order_release);
    resetPlaybackTrackState(*timeline);
    resetRecordingTrackState(*timeline);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::startRecording(const int countInBeats, juce::String& error) noexcept {
    const AudioPublishScope publish(*this);
    if (!publish.isReady()) {
        error = "Arrange recording could not acquire the audio graph boundary.";
        return false;
    }
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr || timeline->outputSampleRate <= 0.0) {
        error = "Arrange recording requires a prepared Arrangement Graph.";
        return false;
    }
    if (recordingPhase.load(std::memory_order_acquire) != RecordingPhase::idle) {
        error = "Arrange recording is already active.";
        return false;
    }
    for (auto& track : timeline->tracks) {
        recordingCapture->resetTrack(track->recordingCapture);
        if (!track->instrument) track->recordingCapture.effectChain.reset();
    }
    recordingCapture->resetDrainingTailTracks();
    recordingCapture->resetCaptureErrors();
    recordingPassOrdinal.store(1, std::memory_order_release);
    const auto alreadyPlaying = state.load(std::memory_order_acquire) == State::playing;
    if (alreadyPlaying || countInBeats <= 0) {
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        recordingStartAudioSample.store(audioClockSample.load(std::memory_order_acquire),
                                        std::memory_order_release);
        const auto tick = timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate);
        recordingStartTick.store(tick, std::memory_order_release);
        if (!alreadyPlaying) state.store(State::playing, std::memory_order_release);
    } else {
        countInRemainingSamples.store(timeline->beatSamples * std::max(0, countInBeats),
                                      std::memory_order_release);
        recordingPhase.store(RecordingPhase::countingIn, std::memory_order_release);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::stopRecording() noexcept {
    recordingPhase.store(RecordingPhase::stopping, std::memory_order_release);
    const AudioPublishScope publish(*this);
    if (!publish.isReady()) {
        sequence.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    const auto hasCaptureWork =
        timeline != nullptr &&
        std::any_of(timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return recordingCapture->hasCaptureWork(track->recordingCapture);
        });
    if (!hasCaptureWork) recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::cancelRecordingIfCountingIn() noexcept {
    auto expected = RecordingPhase::countingIn;
    if (!recordingPhase.compare_exchange_strong(expected, RecordingPhase::idle,
                                                std::memory_order_acq_rel))
        return false;
    countInRemainingSamples.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::flushRecordingTail(juce::String& error) noexcept {
    {
        const AudioPublishScope publish(*this);
        if (!publish.isReady()) {
            error = "Processed recording tail could not acquire the audio graph boundary.";
            return false;
        }
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        auto sinkLease = recordingCapture->acquireSink();
        auto* sink = sinkLease.get();
        if (timeline == nullptr || sink == nullptr) {
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return true;
        }
        if (timeline->loopEnabled) {
            // Loop recording: close any active segment, then generate processed offline
            for (auto& trackPtr : timeline->tracks) {
                auto& track = *trackPtr;
                if (!track.armed || track.instrument ||
                    track.recordingCapture.state != RecordingCaptureState::capturing)
                    continue;
                (void)recordingCapture->endTrackCapture(track.id, track.recordingCapture);
                track.recordingCapture.state = RecordingCaptureState::idle;
            }
            if (!generateLoopProcessedVariants(*timeline, sink)) {
                error = "Loop recording Processed Variant generation failed.";
                return false;
            }
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return recordingCapture->captureErrors() == 0;
        }
        for (auto& trackPtr : timeline->tracks) {
            auto& track = *trackPtr;
            if (!track.armed || track.instrument ||
                track.recordingCapture.state != RecordingCaptureState::capturing)
                continue;
            if (!recordingCapture->beginTailDrain(track.id, track.recordingCapture,
                                                  track.pluginDelaySamples,
                                                  track.pluginTailSamples)) {
                error = "Recording Capture Segment could not be closed for tail drain.";
                return false;
            }
        }
    }

    const auto deadline = juce::Time::getMillisecondCounter() + 5000u;
    while (recordingCapture->drainingTailTracks() != 0) {
        if (juce::Time::getMillisecondCounter() >= deadline) {
            error = "Processed recording tail did not drain before the realtime deadline.";
            return false;
        }
        juce::Thread::sleep(1);
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    return recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::generateLoopProcessedVariants(PreparedTimeline& prepared,
                                                   ArrangementCaptureSink* const sink) noexcept {
    const auto blockSize = std::max(1, prepared.preparedBlockSize);
    juce::AudioFormatManager formatReader;
    formatReader.registerBasicFormats();
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.instrument || !track.armed) continue;
        const auto rawFile = sink->prepareRawForReading(track.id);
        if (rawFile == juce::File{}) continue;
        const auto segments = sink->getRawSegmentRanges(track.id);
        if (segments.empty()) continue;
        // Open the flushed raw file as a stream so that non-.wav extensions
        // (e.g. .partial) are accepted by the AudioFormatManager readers.
        auto rawStream = rawFile.createInputStream();
        if (rawStream == nullptr || !rawStream->openedOk()) return false;
        std::unique_ptr<juce::AudioFormatReader> reader(
            formatReader.createReaderFor(std::move(rawStream)));
        if (reader == nullptr) return false;
        const auto delay = static_cast<int>(std::max<std::int64_t>(0, track.pluginDelaySamples));
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentSamples = static_cast<int>(segEnd - segStart);
            if (segmentSamples <= 0) continue;
            track.recordingCapture.effectChain.reset();
            // Output buffer holds segmentSamples + delay for latency alignment
            const auto outputSize = segmentSamples + delay;
            juce::AudioBuffer<float> outputBuffer(2, outputSize);
            outputBuffer.clear();
            int outputOffset = 0;
            // Process raw segment in block-sized chunks
            juce::AudioBuffer<float> blockBuffer(2, blockSize);
            int remaining = segmentSamples;
            std::int64_t readPos = static_cast<std::int64_t>(segStart);
            while (remaining > 0) {
                const auto count = std::min(blockSize, remaining);
                blockBuffer.clear();
                reader->read(blockBuffer.getArrayOfWritePointers(), 2, readPos, count);
                readPos += count;
                const std::array<float*, 2> outPtrs{
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(blockBuffer.getArrayOfReadPointers(), 2,
                                                           outPtrs.data(), 2, count);
                outputOffset += count;
                remaining -= count;
            }
            // Feed delay zeros to flush plugin latency
            int delayRemaining = delay;
            while (delayRemaining > 0) {
                const auto count = std::min(blockSize, delayRemaining);
                blockBuffer.clear();
                const std::array<float*, 2> outPtrs{
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(blockBuffer.getArrayOfReadPointers(), 2,
                                                           outPtrs.data(), 2, count);
                outputOffset += count;
                delayRemaining -= count;
            }
            // Discard first delay samples, write segmentSamples in block-sized chunks
            constexpr int kOfflineWriterTimeoutMs = 5000;
            int writeOffset = 0;
            while (writeOffset < segmentSamples) {
                const auto count = std::min(blockSize, segmentSamples - writeOffset);
                const std::array<const float*, 2> processedBlock{
                    outputBuffer.getReadPointer(0) + delay + writeOffset,
                    outputBuffer.getReadPointer(1) + delay + writeOffset,
                };
                if (!sink->writeProcessedAudioTrackOffline(track.id, processedBlock.data(), count,
                                                           kOfflineWriterTimeoutMs)) {
                    return false;
                }
                writeOffset += count;
            }
        }
    }
    return true;
}

juce::var TimelineEngine::recordingConfiguration() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return {};
    auto* result = new juce::DynamicObject();
    result->setProperty("sampleRate", timeline->outputSampleRate);
    const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
        timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate));
    result->setProperty("timelineStartTick", tick);
    result->setProperty("loopEnabled", timeline->loopEnabled);
    result->setProperty("loopStartSample", static_cast<juce::int64>(timeline->loopStartSample));
    result->setProperty("loopEndSample", static_cast<juce::int64>(timeline->loopEndSample));
    result->setProperty("punchEnabled", timeline->punchEnabled);
    result->setProperty("punchStartSample", static_cast<juce::int64>(timeline->punchStartSample));
    result->setProperty("punchEndSample", static_cast<juce::int64>(timeline->punchEndSample));
    juce::Array<juce::var> trackValues;
    for (const auto& track : timeline->tracks) {
        if (!track->armed) continue;
        auto* value = new juce::DynamicObject();
        value->setProperty("trackId", track->id);
        value->setProperty("kind", track->instrument ? "instrument" : "audio");
        value->setProperty("audioInputChannel", track->audioInputChannel);
        value->setProperty("midiDeviceId", track->midiDeviceId);
        value->setProperty("midiChannel", track->midiChannel);
        value->setProperty("pluginLatencySamples", static_cast<int>(track->pluginDelaySamples));
        value->setProperty("pluginTailSamples", static_cast<int>(track->pluginTailSamples));
        trackValues.add(juce::var(value));
    }
    result->setProperty("tracks", trackValues);
    return juce::var(result);
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingCapture->setSink(sink);
}

void TimelineEngine::clearRecordingSink() noexcept { recordingCapture->clearSink(); }

bool TimelineEngine::enqueueLiveMidi(const juce::MidiMessage& message,
                                     const juce::String& deviceId) noexcept {
    if (!armedInstrumentTrack.load(std::memory_order_acquire)) return false;
    if (publishInProgress.load(std::memory_order_acquire)) return true;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || publishInProgress.load(std::memory_order_acquire) ||
        timeline == nullptr)
        return true;
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (track.instrument && track.armed &&
            ArrangementGraph::midiRouteMatches(track.midiDeviceId, track.midiChannel, deviceId,
                                               message.getChannel())) {
            if (track.liveInstrumentRack != nullptr) track.liveInstrumentRack->enqueueMidi(message);
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(track.id, deviceId, message,
                                                 audioClockSample.load(std::memory_order_acquire));
            }
        }
    }
    return true;
}

bool TimelineEngine::enqueueTargetedMidi(const juce::String& trackId,
                                         const juce::MidiMessage& message,
                                         juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI input.";
        return false;
    }
    if (publishInProgress.load(std::memory_order_acquire)) {
        error = "The Arrangement Graph is changing; targeted MIDI can be retried shortly.";
        return false;
    }
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || publishInProgress.load(std::memory_order_acquire) ||
        timeline == nullptr) {
        error = "The Arrangement Graph is unavailable for targeted MIDI.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "The target Track is not available in the Arrangement Graph.";
        return false;
    }
    auto& track = **found;
    if (!track.instrument || track.liveInstrumentRack == nullptr ||
        !track.liveInstrumentRack->isLoaded()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    track.liveInstrumentRack->enqueueMidi(message);
    if (track.armed &&
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
        recordingCapture->writeMidiTrack(track.id, "riffra:play-surface", message,
                                         audioClockSample.load(std::memory_order_acquire));
    }
    return true;
}

bool TimelineEngine::panicTargetedMidi(const juce::String& trackId, juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI panic.";
        return false;
    }
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || publishInProgress.load(std::memory_order_acquire) ||
        timeline == nullptr) {
        error = "The Arrangement Graph is unavailable for targeted MIDI panic.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "The target Track is not available in the Arrangement Graph.";
        return false;
    }
    auto& track = **found;
    if (!track.instrument || track.liveInstrumentRack == nullptr ||
        !track.liveInstrumentRack->isLoaded()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    track.liveInstrumentRack->allNotesOff();
    if (track.instrumentRack != nullptr) track.instrumentRack->allNotesOff();
    return true;
}

void TimelineEngine::panicAllInstrumentTracks() noexcept {
    panicAllPending.store(true, std::memory_order_release);
}

void TimelineEngine::servicePendingPanic() noexcept {
    AudioReadScope activeRead(*this);
    if (auto* active = activeRead.get(); active != nullptr) applyPendingPanic(*active);
}

PluginRack* TimelineEngine::findDevice(const juce::String& trackId,
                                       const juce::String& deviceId) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return nullptr;
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) return nullptr;
    auto& track = **found;
    const auto instrument = track.instrumentRack.get();
    if (instrument != nullptr && deviceId == track.instrumentDeviceId) return instrument;
    return track.effectChain.findDevice(deviceId);
}

bool TimelineEngine::mirrorEditorDeviceState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const juce::var& persistedState,
                                             juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId) {
        if (track.liveInstrumentRack != nullptr &&
            !track.liveInstrumentRack->applyPersistedState(persistedState, error))
            return false;
        return true;
    }
    auto* playback = track.effectChain.findDevice(deviceId);
    if (playback == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    auto* live = track.liveEffectChain.findDevice(deviceId);
    if (live != nullptr && !live->applyPersistedState(persistedState, error)) return false;
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (recording != nullptr && !recording->applyPersistedState(persistedState, error))
        return false;
    return true;
}

bool TimelineEngine::mirrorEditorDeviceParameter(const juce::String& trackId,
                                                 const juce::String& deviceId,
                                                 const int parameterIndex, const float value,
                                                 juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId) {
        if (track.liveInstrumentRack != nullptr)
            track.liveInstrumentRack->enqueueParameterChange(parameterIndex, value);
        sequence.fetch_add(1, std::memory_order_relaxed);
        return true;
    }
    auto* playback = track.effectChain.findDevice(deviceId);
    if (playback == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    if (auto* live = track.liveEffectChain.findDevice(deviceId))
        live->enqueueParameterChange(parameterIndex, value);
    if (auto* recording = track.recordingCapture.effectChain.findDevice(deviceId))
        recording->enqueueParameterChange(parameterIndex, value);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

juce::var TimelineEngine::devicePersistedState(const juce::String& trackId,
                                               const juce::String& deviceId,
                                               juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return track.instrumentRack->persistedState(error);
    return track.effectChain.persistedState(deviceId, error);
}

bool TimelineEngine::preparedTrackReusesRuntimeDevices(const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (pendingTimeline == nullptr) return false;
    const auto track = std::find_if(pendingTimeline->tracks.begin(), pendingTimeline->tracks.end(),
                                    [&trackId](const auto& item) { return item->id == trackId; });
    return track != pendingTimeline->tracks.end() && (*track)->reuseRuntimeDevices;
}

bool TimelineEngine::hasPreparedSnapshot() const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return pendingTimeline != nullptr;
}

bool TimelineEngine::setDeviceBypassed(const juce::String& trackId, const juce::String& deviceId,
                                       const bool bypassed, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId) {
        track.instrumentRack->setBypassed(bypassed);
        if (track.liveInstrumentRack != nullptr) track.liveInstrumentRack->setBypassed(bypassed);
    } else {
        auto* playback = track.effectChain.findDevice(deviceId);
        auto* live = track.liveEffectChain.findDevice(deviceId);
        auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
        if (playback == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        playback->setBypassed(bypassed);
        if (live != nullptr) live->setBypassed(bypassed);
        if (recording != nullptr) recording->setBypassed(bypassed);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceParameter(const juce::String& trackId, const juce::String& deviceId,
                                        const int parameterIndex, const float value,
                                        juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    auto* playback = track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId
                         ? track.instrumentRack.get()
                         : track.effectChain.findDevice(deviceId);
    auto* live = track.liveEffectChain.findDevice(deviceId);
    auto* liveInstrument = track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId
                               ? track.liveInstrumentRack.get()
                               : nullptr;
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (playback == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    const auto parameterStatus = playback->parameterStatus().getProperty("parameters", {});
    if (!parameterStatus.isArray() || parameterIndex < 0 ||
        parameterIndex >= parameterStatus.size()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    const auto previous =
        static_cast<float>(parameterStatus[parameterIndex].getProperty("value", 0.0));
    if (!playback->setParameter(parameterIndex, value, error)) return false;
    if (live != nullptr && !live->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void)playback->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    if (liveInstrument != nullptr && !liveInstrument->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void)playback->setParameter(parameterIndex, previous, rollbackError);
        if (live != nullptr) (void)live->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    if (recording != nullptr && !recording->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void)playback->setParameter(parameterIndex, previous, rollbackError);
        if (live != nullptr) (void)live->setParameter(parameterIndex, previous, rollbackError);
        if (liveInstrument != nullptr)
            (void)liveInstrument->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return monitorLiveInput.load(std::memory_order_acquire);
}

bool TimelineEngine::monitoringInputChannel(const int channel) const noexcept {
    if (channel < 0 || channel >= 32) return false;
    const auto channels = monitoringInputChannels.load(std::memory_order_acquire);
    return (channels & (std::uint32_t{1} << static_cast<unsigned>(channel))) != 0;
}

bool TimelineEngine::recordingWindow(const int sampleCount, int& sampleOffset,
                                     int& capturedSamples) noexcept {
    sampleOffset = 0;
    capturedSamples = std::max(0, sampleCount);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    if (sampleCount <= 0) return false;
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    auto phase = recordingPhase.load(std::memory_order_acquire);
    auto transitionedFromCountIn = false;
    if (phase == RecordingPhase::idle || phase == RecordingPhase::stopping) {
        capturedSamples = 0;
        return false;
    }
    if (phase == RecordingPhase::countingIn) {
        const auto remaining = countInRemainingSamples.load(std::memory_order_acquire);
        countInBlockStartRemainingSamples.store(remaining, std::memory_order_release);
        if (remaining >= sampleCount) {
            countInRemainingSamples.store(remaining - sampleCount, std::memory_order_release);
            capturedSamples = 0;
            return false;
        }
        sampleOffset = static_cast<int>(std::max<std::int64_t>(0, remaining));
        playbackBlockOffset.store(sampleOffset, std::memory_order_release);
        capturedSamples = sampleCount - sampleOffset;
        countInRemainingSamples.store(0, std::memory_order_release);
        recordingStartAudioSample.store(audioClockSample.load(std::memory_order_acquire) +
                                            static_cast<std::uint64_t>(sampleOffset),
                                        std::memory_order_release);
        if (active != nullptr) {
            const auto tick = active->timebase.sampleToTick(
                timelineSample.load(std::memory_order_acquire), active->outputSampleRate);
            recordingStartTick.store(tick, std::memory_order_release);
        }
        state.store(State::playing, std::memory_order_release);
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        phase = RecordingPhase::recording;
        transitionedFromCountIn = true;
    }

    if (active == nullptr || !active->punchEnabled) {
        captureBlockOffset.store(sampleOffset, std::memory_order_release);
        captureBlockSamples.store(capturedSamples, std::memory_order_release);
        return true;
    }

    const auto position = timelineSample.load(std::memory_order_acquire);
    const auto playbackOffset = transitionedFromCountIn ? sampleOffset : 0;
    const auto playbackSamples = sampleCount - playbackOffset;
    const auto blockEnd = position + static_cast<std::int64_t>(playbackSamples);
    if (blockEnd <= active->punchStartSample || position >= active->punchEndSample) {
        capturedSamples = 0;
        return false;
    }
    const auto punchOffset =
        static_cast<int>(std::max<std::int64_t>(0, active->punchStartSample - position));
    sampleOffset = playbackOffset + punchOffset;
    const auto end = std::min<std::int64_t>(blockEnd, active->punchEndSample);
    capturedSamples = static_cast<int>(std::max<std::int64_t>(0, end - position - punchOffset));
    captureBlockOffset.store(sampleOffset, std::memory_order_release);
    captureBlockSamples.store(capturedSamples, std::memory_order_release);
    return capturedSamples > 0;
}

void TimelineEngine::mixMetronome(float* const* outputChannels, const int channelCount,
                                  const int sampleCount) noexcept {
    if (sampleCount <= 0) return;
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    if (active == nullptr || !active->metronomeEnabled || active->beatSamples <= 0) return;
    const auto loopLength = active->loopEndSample - active->loopStartSample;
    const auto start = lastMixStartSample.load(std::memory_order_acquire);
    const auto playbackOffset =
        juce::jlimit(0, sampleCount, lastMixPlaybackOffset.load(std::memory_order_acquire));
    const auto countInRemaining = countInBlockStartRemainingSamples.load(std::memory_order_acquire);
    const auto countingIn =
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::countingIn;
    const auto playing = state.load(std::memory_order_acquire) == State::playing;
    constexpr std::int64_t clickSamples = 1'920;
    for (int sample = 0; sample < sampleCount; ++sample) {
        float value = 0.0f;
        if (countInRemaining > 0 && sample < (countingIn ? sampleCount : playbackOffset)) {
            const auto remaining = countInRemaining - sample;
            const auto offset =
                (active->beatSamples - remaining % active->beatSamples) % active->beatSamples;
            if (offset >= 0 && offset < clickSamples) {
                const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                value = 0.11f * envelope;
            }
        } else if (playing && sample >= playbackOffset) {
            auto position = start + sample - playbackOffset;
            if (active->loopEnabled && loopLength > 0 && position >= active->loopEndSample)
                position =
                    active->loopStartSample + (position - active->loopEndSample) % loopLength;
            if (position >= 0) {
                const auto beat = position / active->beatSamples;
                const auto offset = position % active->beatSamples;
                if (offset >= 0 && offset < clickSamples) {
                    const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                    const auto amplitude = beat % active->beatsPerBar == 0 ? 0.18f : 0.11f;
                    value = amplitude * envelope;
                }
            }
        }
        if (value <= 0.0f) continue;
        for (int channel = 0; channel < channelCount; ++channel) {
            if (outputChannels[channel] != nullptr) outputChannels[channel][sample] += value;
        }
    }
}

void TimelineEngine::mixRange(Track& track, const std::int64_t rangeStart,
                              const int destinationStart, const int sampleCount) noexcept {
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : track.clips) {
        auto& clip = *clipPtr;
        if (clip.muted) continue;
        const auto clipEnd = clip.startSample + clip.durationSamples;
        const auto overlapStart = std::max(rangeStart, clip.startSample);
        const auto overlapEnd = std::min(rangeEnd, clipEnd);
        if (overlapEnd <= overlapStart) continue;
        auto remaining = static_cast<int>(overlapEnd - overlapStart);
        auto outputOffset = destinationStart + static_cast<int>(overlapStart - rangeStart);
        auto localSample = overlapStart - clip.startSample;
        while (remaining > 0) {
            const auto sourceRange = clip.sourceEndFrame - clip.sourceStartFrame;
            auto sourceOffset = static_cast<std::int64_t>(std::floor(
                static_cast<double>(localSample) * clip.sourceSampleRate / track.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd =
                static_cast<int>(std::ceil(static_cast<double>(sourceRemaining) *
                                           track.outputSampleRate / clip.sourceSampleRate));
            const auto chunk = std::min(remaining, std::max(1, outputUntilSourceEnd));
            if (clip.expectedSourceFrame < 0 ||
                std::abs(clip.expectedSourceFrame - sourceFrame) > 2) {
                clip.positionableSource->setNextReadPosition(sourceFrame);
                clip.resamplingSource->flushBuffers();
            }
            clip.scratch.clear();
            clip.resamplingSource->getNextAudioBlock(
                juce::AudioSourceChannelInfo(&clip.scratch, 0, chunk));
            for (int sample = 0; sample < chunk; ++sample) {
                const auto position = localSample + sample;
                auto envelope = 1.0f;
                if (clip.fadeInSamples > 0 && position < clip.fadeInSamples) {
                    const auto progress =
                        static_cast<float>(position) / static_cast<float>(clip.fadeInSamples);
                    envelope = std::min(envelope, fadeEnvelope(progress, clip.fadeShape));
                }
                const auto remainingClip = clip.durationSamples - position - 1;
                if (clip.fadeOutSamples > 0 && remainingClip < clip.fadeOutSamples) {
                    const auto progress =
                        static_cast<float>(std::max<std::int64_t>(0, remainingClip)) /
                        static_cast<float>(clip.fadeOutSamples);
                    envelope = std::min(envelope, fadeEnvelope(progress, clip.fadeShape));
                }
                const auto panAngle = (clip.pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
                const auto source = clip.scratch.getSample(0, sample) * clip.gain * envelope;
                track.mixBuffer.addSample(0, outputOffset + sample, source * std::cos(panAngle));
                track.mixBuffer.addSample(1, outputOffset + sample,
                                          clip.scratch.getNumChannels() > 1
                                              ? clip.scratch.getSample(1, sample) * clip.gain *
                                                    envelope * std::sin(panAngle)
                                              : source * std::sin(panAngle));
            }
            clip.expectedSourceFrame =
                sourceFrame +
                static_cast<std::int64_t>(std::floor(
                    static_cast<double>(chunk) * clip.sourceSampleRate / track.outputSampleRate));
            remaining -= chunk;
            outputOffset += chunk;
            localSample += chunk;
            if (!clip.loop && sourceFrame + sourceRemaining >= clip.sourceEndFrame && remaining > 0)
                break;
            if (clip.loop && remaining > 0) clip.expectedSourceFrame = -1;
        }
    }
}

void TimelineEngine::scheduleMidi(const PreparedTimeline& prepared, Track& track,
                                  const std::int64_t rangeStart, const int sampleCount) noexcept {
    track.midiBuffer.clear();
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : track.midiClips) {
        if (clip.muted) continue;
        const auto clipStart =
            prepared.timebase.tickToSample(clip.startTick, track.outputSampleRate);
        const auto clipLength = std::max<std::int64_t>(
            1, prepared.timebase.tickToSample(clip.durationTicks, track.outputSampleRate));
        const auto firstIteration =
            clip.loop && rangeStart > clipStart
                ? std::max<std::int64_t>(0, (rangeStart - clipStart) / clipLength - 1)
                : 0;
        const auto lastIteration =
            clip.loop
                ? std::max<std::int64_t>(firstIteration, (rangeEnd - clipStart) / clipLength + 1)
                : 0;
        const auto addMessage = [&](const juce::MidiMessage& message, const std::int64_t sample) {
            if (sample >= rangeStart && sample < rangeEnd)
                track.midiBuffer.addEvent(
                    message,
                    juce::jlimit(0, sampleCount - 1, static_cast<int>(sample - rangeStart)));
        };
        for (std::int64_t iteration = firstIteration; iteration <= lastIteration; ++iteration) {
            const auto iterationStart = clipStart + iteration * clipLength;
            for (const auto& note : clip.notes) {
                const auto noteStart = iterationStart + prepared.timebase.tickToSample(
                                                            note.startTick, track.outputSampleRate);
                const auto noteEnd =
                    std::min(iterationStart + clipLength,
                             noteStart + std::max<std::int64_t>(
                                             1, prepared.timebase.tickToSample(
                                                    note.durationTicks, track.outputSampleRate)));
                addMessage(juce::MidiMessage::noteOn(
                               juce::jlimit(1, 16, note.channel), note.note,
                               static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))),
                           noteStart);
                addMessage(juce::MidiMessage::noteOff(juce::jlimit(1, 16, note.channel), note.note),
                           noteEnd);
            }
            for (const auto& event : clip.events) {
                const auto eventSample = iterationStart + prepared.timebase.tickToSample(
                                                              event.tick, track.outputSampleRate);
                const auto channel = juce::jlimit(1, 16, event.channel);
                if (event.kind == "controlChange")
                    addMessage(
                        juce::MidiMessage::controllerEvent(channel, event.data1, event.data2),
                        eventSample);
                else if (event.kind == "pitchBend")
                    addMessage(
                        juce::MidiMessage::pitchWheel(channel, event.data1 | (event.data2 << 7)),
                        eventSample);
                else if (event.kind == "channelPressure")
                    addMessage(juce::MidiMessage::channelPressureChange(channel, event.data1),
                               eventSample);
            }
            if (!clip.loop) break;
        }
    }
}

void TimelineEngine::processTracks(PreparedTimeline& prepared,
                                   const float* const* physicalInputChannels,
                                   const int physicalInputChannelCount,
                                   float* const* outputChannels, const int channelCount,
                                   const std::int64_t rangeStart, const int destinationStart,
                                   const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(prepared.tracks.begin(), prepared.tracks.end(),
                                     [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        const auto audible = !track.muted && (!hasSolo || track.solo);
        track.processedBuffer.clear(0, sampleCount);
        const float* inputChannels[2] = {track.mixBuffer.getWritePointer(0),
                                         track.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {track.processedBuffer.getWritePointer(0),
                                       track.processedBuffer.getWritePointer(1)};
        if (track.instrument) {
            processInstrumentTrack(track, sampleCount, &track.midiBuffer);
            processLiveInstrumentTrack(track, sampleCount);
        } else {
            track.effectChain.process(inputChannels, 2, processedChannels, 2, sampleCount);
        }
        mixProcessedTrack(track, audible, outputChannels, channelCount, rangeStart,
                          destinationStart, sampleCount);
        if (track.instrument) {
            mixLiveTrack(track, audible, outputChannels, channelCount, rangeStart, destinationStart,
                         sampleCount);
        }
    }
    processLiveAudioTracks(prepared, physicalInputChannels, physicalInputChannelCount,
                           outputChannels, channelCount, rangeStart, destinationStart, sampleCount);
}

void TimelineEngine::processLiveAudioTracks(
    PreparedTimeline& prepared, const float* const* physicalInputChannels,
    const int physicalInputChannelCount, float* const* outputChannels, const int channelCount,
    const std::int64_t rangeStart, const int destinationStart, const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(prepared.tracks.begin(), prepared.tracks.end(),
                                     [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        const auto audible = !track.muted && (!hasSolo || track.solo);
        if (!track.instrument && (track.monitorInput || track.armed) &&
            track.audioInputChannel >= 0) {
            const auto* source = ArrangementGraph::audioInputSource(
                track.audioInputChannel, physicalInputChannels, physicalInputChannelCount);
            for (int channel = 0; channel < 2; ++channel) {
                auto* destination = track.liveInputBuffer.getWritePointer(channel);
                if (source != nullptr)
                    juce::FloatVectorOperations::copy(destination, source + destinationStart,
                                                      sampleCount);
                else
                    juce::FloatVectorOperations::clear(destination, sampleCount);
            }
            if (track.monitorInput) {
                track.liveEffectChain.process(track.liveInputBuffer.getArrayOfReadPointers(), 2,
                                              track.liveProcessedBuffer.getArrayOfWritePointers(),
                                              2, sampleCount);
            }
            const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
            const auto captureEnd =
                captureStart + captureBlockSamples.load(std::memory_order_acquire);
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (track.armed) {
                auto& capture = track.recordingCapture;
                if (writeEnd > writeStart) {
                    const auto localOffset = writeStart - destinationStart;
                    const auto captureAudioStart =
                        callbackAudioStartSample.load(std::memory_order_acquire) +
                        static_cast<std::uint64_t>(writeStart);
                    const auto captureTimelineStart =
                        static_cast<std::uint64_t>(rangeStart + localOffset);
                    const auto discontinuous = capture.state != RecordingCaptureState::capturing ||
                                               captureAudioStart != capture.endAudioSample;
                    if (discontinuous && capture.state == RecordingCaptureState::capturing) {
                        if (!recordingCapture->beginTailDrain(track.id, capture,
                                                              track.pluginDelaySamples,
                                                              track.pluginTailSamples))
                            capture.state = RecordingCaptureState::completed;
                    }
                    if (capture.state == RecordingCaptureState::idle) {
                        capture.effectChain.reset();
                        capture.latencyToDiscard =
                            static_cast<int>(std::max<std::int64_t>(0, track.pluginDelaySamples));
                        (void)recordingCapture->beginTrackCapture(
                            track.id, capture, captureAudioStart, captureTimelineStart);
                    }
                    if (capture.state == RecordingCaptureState::capturing) {
                        const auto writeCount = writeEnd - writeStart;
                        const auto* rawPointer =
                            track.liveInputBuffer.getReadPointer(0) + localOffset;
                        if (prepared.loopEnabled) {
                            // Loop recording: write raw only (processed generated offline)
                            recordingCapture->writeAudioTrack(track.id, rawPointer, writeCount,
                                                              nullptr, 0);
                        } else {
                            // Normal recording: write raw + processed in real-time
                            capture.processedBuffer.clear(0, writeCount);
                            const std::array<const float*, 2> recordingInput{
                                track.liveInputBuffer.getReadPointer(0) + localOffset,
                                track.liveInputBuffer.getReadPointer(1) + localOffset,
                            };
                            capture.effectChain.process(
                                recordingInput.data(), 2,
                                capture.processedBuffer.getArrayOfWritePointers(), 2, writeCount);
                            const auto discard = std::min(capture.latencyToDiscard, writeCount);
                            capture.latencyToDiscard -= discard;
                            const auto processedCount = writeCount - discard;
                            const std::array<const float*, 2> processed{
                                capture.processedBuffer.getReadPointer(0) + discard,
                                capture.processedBuffer.getReadPointer(1) + discard,
                            };
                            recordingCapture->writeAudioTrack(track.id, rawPointer, writeCount,
                                                              processed.data(), processedCount);
                        }
                        capture.endAudioSample =
                            captureAudioStart + static_cast<std::uint64_t>(writeCount);
                        capture.endTimelineSample =
                            captureTimelineStart + static_cast<std::uint64_t>(writeCount);
                    }
                } else if (capture.state == RecordingCaptureState::capturing) {
                    if (!recordingCapture->beginTailDrain(
                            track.id, capture, track.pluginDelaySamples, track.pluginTailSamples))
                        capture.state = RecordingCaptureState::completed;
                }
            }
            if (track.monitorInput && audible) {
                for (int sample = 0; sample < sampleCount; ++sample) {
                    const auto timelinePosition = rangeStart + sample;
                    const auto gain =
                        juce::Decibels::decibelsToGain(ArrangementGraph::automationValueAt(
                            track.volumeAutomation, timelinePosition, track.gainDb));
                    const auto pan =
                        juce::jlimit(-1.0f, 1.0f,
                                     ArrangementGraph::automationValueAt(
                                         track.panAutomation, timelinePosition, track.pan));
                    const auto panAngle = (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
                    if (channelCount > 0 && outputChannels[0] != nullptr)
                        outputChannels[0][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(0, sample) * gain *
                            std::cos(panAngle);
                    if (channelCount > 1 && outputChannels[1] != nullptr)
                        outputChannels[1][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(1, sample) * gain *
                            std::sin(panAngle);
                }
            }
        }
    }
}

void TimelineEngine::processLiveInstrumentTracks(PreparedTimeline& prepared,
                                                 float* const* outputChannels,
                                                 const int channelCount,
                                                 const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(prepared.tracks.begin(), prepared.tracks.end(),
                                     [](const auto& track) { return track->solo; });
    const auto rangeStart = timelineSample.load(std::memory_order_acquire);
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (!track.instrument) continue;
        const auto audible = !track.muted && (!hasSolo || track.solo);
        processLiveInstrumentTrack(track, sampleCount);
        mixLiveTrack(track, audible, outputChannels, channelCount, rangeStart, 0, sampleCount);
    }
}

void TimelineEngine::processInstrumentTrack(Track& track, const int sampleCount,
                                            const juce::MidiBuffer* const timelineMidi) noexcept {
    if (track.instrumentRack != nullptr) {
        track.instrumentRack->process(nullptr, 0, track.mixBuffer.getArrayOfWritePointers(), 2,
                                      sampleCount, timelineMidi);
    } else {
        track.mixBuffer.clear(0, sampleCount);
    }
    track.effectChain.process(track.mixBuffer.getArrayOfReadPointers(), 2,
                              track.processedBuffer.getArrayOfWritePointers(), 2, sampleCount);
}

void TimelineEngine::processLiveInstrumentTrack(Track& track, const int sampleCount) noexcept {
    if (track.liveInstrumentRack != nullptr) {
        track.liveInstrumentRack->process(
            nullptr, 0, track.liveInputBuffer.getArrayOfWritePointers(), 2, sampleCount);
    } else {
        track.liveInputBuffer.clear(0, sampleCount);
    }
    track.liveEffectChain.process(track.liveInputBuffer.getArrayOfReadPointers(), 2,
                                  track.liveProcessedBuffer.getArrayOfWritePointers(), 2,
                                  sampleCount);
}

void TimelineEngine::mixProcessedTrack(Track& track, const bool audible,
                                       float* const* outputChannels, const int channelCount,
                                       const std::int64_t rangeStart, const int destinationStart,
                                       const int sampleCount) noexcept {
    const auto delay = track.compensationDelaySamples;
    const auto delaySize = track.delayBuffer.getNumSamples();
    for (int sample = 0; sample < sampleCount; ++sample) {
        const auto timelinePosition = rangeStart + sample;
        const auto gain = juce::Decibels::decibelsToGain(ArrangementGraph::automationValueAt(
            track.volumeAutomation, timelinePosition, track.gainDb));
        const auto pan = juce::jlimit(
            -1.0f, 1.0f,
            ArrangementGraph::automationValueAt(track.panAutomation, timelinePosition, track.pan));
        const auto panAngle = (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const auto leftGain = gain * std::cos(panAngle);
        const auto rightGain = gain * std::sin(panAngle);
        float left = track.processedBuffer.getSample(0, sample);
        float right = track.processedBuffer.getSample(1, sample);
        if (delay > 0 && delaySize > 0) {
            const auto write = track.delayWritePosition;
            track.delayBuffer.setSample(0, static_cast<int>(write), left);
            track.delayBuffer.setSample(1, static_cast<int>(write), right);
            const auto read = (write - delay + delaySize) % delaySize;
            left = track.delayBuffer.getSample(0, static_cast<int>(read));
            right = track.delayBuffer.getSample(1, static_cast<int>(read));
            track.delayWritePosition = (write + 1) % delaySize;
        }
        if (audible && channelCount > 0 && outputChannels[0] != nullptr)
            outputChannels[0][destinationStart + sample] += left * leftGain;
        if (audible && channelCount > 1 && outputChannels[1] != nullptr)
            outputChannels[1][destinationStart + sample] += right * rightGain;
    }
}

void TimelineEngine::mixLiveTrack(Track& track, const bool audible, float* const* outputChannels,
                                  const int channelCount, const std::int64_t rangeStart,
                                  const int destinationStart, const int sampleCount) noexcept {
    for (int sample = 0; sample < sampleCount; ++sample) {
        const auto timelinePosition = rangeStart + sample;
        const auto gain = juce::Decibels::decibelsToGain(ArrangementGraph::automationValueAt(
            track.volumeAutomation, timelinePosition, track.gainDb));
        const auto pan = juce::jlimit(
            -1.0f, 1.0f,
            ArrangementGraph::automationValueAt(track.panAutomation, timelinePosition, track.pan));
        const auto panAngle = (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const float left = track.liveProcessedBuffer.getSample(0, sample);
        const float right = track.liveProcessedBuffer.getSample(1, sample);
        if (audible && channelCount > 0 && outputChannels[0] != nullptr)
            outputChannels[0][destinationStart + sample] += left * gain * std::cos(panAngle);
        if (audible && channelCount > 1 && outputChannels[1] != nullptr)
            outputChannels[1][destinationStart + sample] += right * gain * std::sin(panAngle);
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        track.mixBuffer.clear();
        track.processedBuffer.clear();
        track.midiBuffer.clear();
        if (track.instrumentRack != nullptr) track.instrumentRack->allNotesOff();
        if (track.liveInstrumentRack != nullptr) track.liveInstrumentRack->allNotesOff();
        track.effectChain.allNotesOff();
        track.liveEffectChain.allNotesOff();
        track.delayBuffer.clear();
        track.delayWritePosition = 0;
    }
}

void TimelineEngine::applyPendingPanic(PreparedTimeline& prepared) noexcept {
    if (!panicAllPending.exchange(false, std::memory_order_acq_rel)) return;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.instrumentRack != nullptr) track.instrumentRack->allNotesOff();
        if (track.liveInstrumentRack != nullptr) track.liveInstrumentRack->allNotesOff();
        track.effectChain.allNotesOff();
        track.liveEffectChain.allNotesOff();
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.recordingCapture);
    }
    recordingCapture->resetDrainingTailTracks();
}

void TimelineEngine::mix(float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    mix(nullptr, 0, outputChannels, channelCount, sampleCount);
}

void TimelineEngine::mix(const float* const* inputChannels, const int inputChannelCount,
                         float* const* outputChannels, const int channelCount,
                         const int sampleCount) noexcept {
    audioClockSample.fetch_add(static_cast<std::uint64_t>(sampleCount), std::memory_order_relaxed);
    callbackAudioStartSample.store(
        audioClockSample.load(std::memory_order_acquire) - static_cast<std::uint64_t>(sampleCount),
        std::memory_order_release);
    const auto blockPlaybackOffset =
        juce::jlimit(0, sampleCount, playbackBlockOffset.exchange(0, std::memory_order_acq_rel));
    lastMixPlaybackOffset.store(blockPlaybackOffset, std::memory_order_release);
    AudioReadScope activeRead(*this);
    auto* active = activeRead.get();
    if (active == nullptr) return;
    applyPendingPanic(*active);
    const auto currentState = state.load(std::memory_order_acquire);
    if (currentState == State::stopped) {
        processLiveInstrumentTracks(*active, outputChannels, channelCount, sampleCount);
        processLiveAudioTracks(*active, inputChannels, inputChannelCount, outputChannels,
                               channelCount, timelineSample.load(std::memory_order_acquire), 0,
                               sampleCount);
        return;
    }
    if (currentState != State::playing) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    if (recordingCapture->drainingTailTracks() != 0) {
        for (auto& trackPtr : active->tracks)
            (void)recordingCapture->drainTail(trackPtr->id, trackPtr->recordingCapture,
                                              trackPtr->liveInputBuffer, sampleCount);
    }
    if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::stopping) {
        if (recordingCapture->drainingTailTracks() != 0) return;
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
        return;
    }
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = sampleCount - consumed;
        if (!active->tracks.empty()) {
            const auto bufferSize = active->tracks.front()->mixBuffer.getNumSamples();
            if (bufferSize > 0) chunk = std::min(chunk, bufferSize);
        }
        if (active->loopEnabled && position < active->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(active->loopEndSample - position));
        for (auto& trackPtr : active->tracks) trackPtr->mixBuffer.clear(0, chunk);
        for (auto& trackPtr : active->tracks) mixRange(*trackPtr, position, 0, chunk);
        for (auto& trackPtr : active->tracks) scheduleMidi(*active, *trackPtr, position, chunk);
        const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
        const auto captureSamples = captureBlockSamples.load(std::memory_order_acquire);
        const auto [captureWriteStart, captureWriteEnd] =
            ArrangementGraph::captureIntersection(consumed, chunk, captureStart, captureSamples);
        if (captureWriteEnd > captureWriteStart &&
            recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
            const auto callbackStart = audioClockSample.load(std::memory_order_acquire) -
                                       static_cast<std::uint64_t>(sampleCount);
            const auto localOffset = captureWriteStart - consumed;
            recordingCapture->setCaptureRange(
                callbackStart + static_cast<std::uint64_t>(captureWriteStart),
                callbackStart + static_cast<std::uint64_t>(captureWriteEnd),
                static_cast<std::uint64_t>(position) + static_cast<std::uint64_t>(localOffset),
                static_cast<std::uint64_t>(position) +
                    static_cast<std::uint64_t>(localOffset + captureWriteEnd - captureWriteStart));
        }
        processTracks(*active, inputChannels, inputChannelCount, outputChannels, channelCount,
                      position, consumed, chunk);
        position += chunk;
        consumed += chunk;
        // Decrement the capture budget so recording stops at the window end
        {
            auto remaining = captureBlockSamples.load(std::memory_order_acquire);
            if (remaining > 0)
                captureBlockSamples.store(remaining - std::min(chunk, remaining),
                                          std::memory_order_release);
        }
        if (active->loopEnabled && position >= active->loopEndSample) {
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                const auto callbackStart = audioClockSample.load(std::memory_order_acquire) -
                                           static_cast<std::uint64_t>(sampleCount);
                recordingCapture->markLoopBoundary(callbackStart +
                                                   static_cast<std::uint64_t>(consumed));
                for (auto& trackPtr : active->tracks) {
                    auto& track = *trackPtr;
                    if (!track.armed || track.instrument ||
                        track.recordingCapture.state != RecordingCaptureState::capturing)
                        continue;
                    (void)recordingCapture->endTrackCapture(track.id, track.recordingCapture);
                    track.recordingCapture.state = RecordingCaptureState::idle;
                    track.recordingCapture.effectChain.reset();
                    track.recordingCapture.latencyToDiscard = 0;
                }
            }
            position = active->loopStartSample;
            recordingPassOrdinal.fetch_add(1, std::memory_order_relaxed);
            resetPlaybackTrackState(*active);
            discontinuity.fetch_add(1, std::memory_order_relaxed);
        }
    }
    timelineSample.store(position, std::memory_order_release);
}

juce::var TimelineEngine::status() const {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "transportStatus");
    const auto currentState = state.load(std::memory_order_acquire);
    object->setProperty("state", currentState == State::playing   ? "playing"
                                 : currentState == State::faulted ? "faulted"
                                                                  : "stopped");
    object->setProperty("timelineSample",
                        static_cast<juce::int64>(timelineSample.load(std::memory_order_acquire)));
    object->setProperty("audioClockSample",
                        static_cast<juce::int64>(audioClockSample.load(std::memory_order_acquire)));
    object->setProperty(
        "sequence", static_cast<juce::int64>(sequence.fetch_add(1, std::memory_order_relaxed) + 1));
    object->setProperty(
        "callbackLockMisses",
        static_cast<juce::int64>(callbackLockMisses.load(std::memory_order_acquire)));
    object->setProperty(
        "callbackPublishMisses",
        static_cast<juce::int64>(callbackPublishMisses.load(std::memory_order_acquire)));
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
    object->setProperty("drainingTailTracks",
                        static_cast<int>(recordingCapture->drainingTailTracks()));
    object->setProperty("unavailableClipIds", juce::Array<juce::var>{});
    object->setProperty("missingDeviceIds", juce::Array<juce::var>{});
    juce::Array<juce::var> armedTrackIds;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire), timeline->outputSampleRate));
        object->setProperty("timelineTick", tick);
        object->setProperty("recordingCurrentTick", tick);
        object->setProperty("unavailableClipIds", timeline->unavailableClipIds);
        object->setProperty("missingDeviceIds", timeline->missingDeviceIds);
        for (const auto& track : timeline->tracks)
            if (track->armed) armedTrackIds.add(track->id);
    }
    object->setProperty("armedTrackIds", armedTrackIds);
    return juce::var(object);
}

}  // namespace riffra
