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
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    for (auto& track : timeline->tracks) {
        recordingCapture->resetTrack(track->runtime->recordingCapture);
    }
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
            return recordingCapture->hasCaptureWork(track->runtime->recordingCapture);
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

bool TimelineEngine::finalizeRecording(juce::String& error) noexcept {
    const AudioPublishScope publish(*this);
    if (!publish.isReady()) {
        error = "Recording capture could not acquire the audio graph boundary.";
        return false;
    }
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    auto sinkLease = recordingCapture->acquireSink();
    auto* sink = sinkLease.get();
    finalizedRecordingTracks.clear();
    finalizedRecordingSampleRate = 0.0;
    finalizedRecordingBlockSize = 0;
    if (timeline == nullptr || sink == nullptr) {
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
        return true;
    }
    finalizedRecordingSampleRate = timeline->outputSampleRate;
    finalizedRecordingBlockSize = timeline->preparedBlockSize;
    finalizedRecordingTracks.reserve(timeline->tracks.size());
    for (const auto& track : timeline->tracks) {
        if (track->runtime == nullptr || track->runtime->instrumentTrack || !track->runtime->armed)
            continue;
        finalizedRecordingTracks.push_back({track->id, track->effectState});
    }
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (!track.runtime->armed || track.runtime->instrumentTrack ||
            track.runtime->recordingCapture.state != RecordingCaptureState::capturing)
            continue;
        if (!recordingCapture->endTrackCapture(track.id, track.runtime->recordingCapture)) {
            error = "Recording Capture Segment could not be closed.";
            track.runtime->recordingCapture.state = RecordingCaptureState::idle;
            finalizedRecordingTracks.clear();
            finalizedRecordingSampleRate = 0.0;
            finalizedRecordingBlockSize = 0;
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return false;
        }
        track.runtime->recordingCapture.state = RecordingCaptureState::idle;
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    return recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::processFinalizedRecording(juce::String& error) noexcept {
    return processFinalizedRecording(nullptr, error);
}

bool TimelineEngine::processFinalizedRecording(ArrangementCaptureSink* sink,
                                               juce::String& error) noexcept {
    std::vector<OfflineRecordingTrack> tracks;
    double sampleRate;
    int blockSize;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        sampleRate = finalizedRecordingSampleRate;
        blockSize = finalizedRecordingBlockSize;
        tracks = std::move(finalizedRecordingTracks);
        finalizedRecordingSampleRate = 0.0;
        finalizedRecordingBlockSize = 0;
    }

    if (sink == nullptr) {
        auto sinkLease = recordingCapture->acquireSink();
        sink = sinkLease.get();
        if (sink == nullptr) return true;
        const auto generated =
            generateProcessedVariants(sampleRate, blockSize, tracks, sink, error);
        return generated && recordingCapture->captureErrors() == 0;
    }
    const auto generated = generateProcessedVariants(sampleRate, blockSize, tracks, sink, error);
    return generated && recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::generateProcessedVariants(const double sampleRate, const int preparedBlockSize,
                                               const std::vector<OfflineRecordingTrack>& tracks,
                                               ArrangementCaptureSink* const sink,
                                               juce::String& error) noexcept {
    if (sink == nullptr || sampleRate <= 0.0) return true;
    const auto blockSize = std::max(1, preparedBlockSize);
    juce::AudioFormatManager formatReader;
    formatReader.registerBasicFormats();
    for (const auto& track : tracks) {
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
        if (reader == nullptr) {
            error = "Recorded raw audio could not be opened for offline processing.";
            return false;
        }
        PluginChain offlineEffects;
        if (!offlineEffects.load(track.effectState, sampleRate, blockSize, error,
                                 track.id + "/offline-processing"))
            return false;
        const auto delay = std::max(0, offlineEffects.latencySamples());
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentLength = segEnd - segStart;
            if (segmentLength > static_cast<std::uint64_t>(std::numeric_limits<int>::max())) {
                error = "Recorded audio segment is too large for offline processing.";
                return false;
            }
            const auto segmentSamples = static_cast<int>(segmentLength);
            if (segmentSamples <= 0) continue;
            offlineEffects.reset();
            juce::AudioBuffer<float> blockBuffer(2, blockSize);
            juce::AudioBuffer<float> processedBlock(2, blockSize);
            int discarded = delay;
            int written = 0;
            constexpr int kOfflineWriterTimeoutMs = 5000;
            const auto consumeProcessedBlock = [&](const int count) noexcept {
                auto writeOffset = 0;
                if (discarded > 0) {
                    const auto skipped = std::min(discarded, count);
                    discarded -= skipped;
                    writeOffset += skipped;
                }
                const auto writable = std::min(count - writeOffset, segmentSamples - written);
                if (writable <= 0) return true;
                const std::array<const float*, 2> outputChannels{
                    processedBlock.getReadPointer(0) + writeOffset,
                    processedBlock.getReadPointer(1) + writeOffset,
                };
                if (!sink->writeProcessedAudioTrackOffline(track.id, outputChannels.data(),
                                                           writable, kOfflineWriterTimeoutMs))
                    return false;
                written += writable;
                return true;
            };

            // Process raw audio in bounded blocks and write post-latency samples immediately.
            int remaining = segmentSamples;
            std::int64_t readPos = static_cast<std::int64_t>(segStart);
            while (remaining > 0) {
                const auto count = std::min(blockSize, remaining);
                blockBuffer.clear();
                if (!reader->read(blockBuffer.getArrayOfWritePointers(), 2, readPos, count)) {
                    error = "Recorded raw audio could not be read for offline processing.";
                    return false;
                }
                readPos += count;
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count);
                if (!consumeProcessedBlock(count)) return false;
                remaining -= count;
            }

            // Flush plugin latency with bounded zero blocks until the segment length is written.
            while (written < segmentSamples) {
                const auto count = blockSize;
                blockBuffer.clear();
                offlineEffects.process(blockBuffer.getArrayOfReadPointers(), 2,
                                       processedBlock.getArrayOfWritePointers(), 2, count);
                if (!consumeProcessedBlock(count)) return false;
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
        if (!track->runtime->armed) continue;
        auto* value = new juce::DynamicObject();
        value->setProperty("trackId", track->id);
        value->setProperty("kind", track->runtime->instrumentTrack ? "instrument" : "audio");
        value->setProperty("audioInputChannel", track->runtime->audioInputChannel);
        value->setProperty("midiDeviceId", track->runtime->midiDeviceId);
        value->setProperty("midiChannel", track->runtime->midiChannel);
        value->setProperty("pluginLatencySamples",
                           static_cast<int>(track->runtime->pluginDelaySamples));
        value->setProperty("pluginTailSamples",
                           static_cast<int>(track->runtime->pluginTailSamples));
        trackValues.add(juce::var(value));
    }
    result->setProperty("tracks", trackValues);
    return juce::var(result);
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingCapture->setSink(sink);
}

void TimelineEngine::clearRecordingSink() noexcept { recordingCapture->clearSink(); }

bool TimelineEngine::setLiveMidiTarget(const juce::String& trackId, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (trackId.isNotEmpty()) {
        const auto isInstrumentTrack = [&trackId](const std::unique_ptr<Track>& track) {
            return track->id == trackId && track->runtime != nullptr &&
                   track->runtime->instrumentTrack;
        };
        const auto foundInTimeline =
            timeline != nullptr &&
            std::any_of(timeline->tracks.begin(), timeline->tracks.end(), isInstrumentTrack);
        const auto foundInPending = pendingTimeline != nullptr &&
                                    std::any_of(pendingTimeline->tracks.begin(),
                                                pendingTimeline->tracks.end(), isInstrumentTrack);
        if (!foundInTimeline && !foundInPending) {
            error = "Live MIDI target must be an Instrument Track.";
            return false;
        }
    }
    liveMidiTargetTrackId = trackId;
    const auto apply = [this](PreparedTimeline* prepared) {
        if (prepared == nullptr) return;
        for (auto& track : prepared->tracks) {
            auto& runtime = *track->runtime;
            runtime.setLowLatencyMonitoring(
                runtime.instrumentTrack ? (runtime.armed || track->id == liveMidiTargetTrackId)
                                        : runtime.monitorInput);
        }
    };
    apply(timeline.get());
    apply(pendingTimeline.get());
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

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
        if (track.runtime->instrumentTrack && track.runtime->armed &&
            ArrangementGraph::midiRouteMatches(track.runtime->midiDeviceId,
                                               track.runtime->midiChannel, deviceId,
                                               message.getChannel())) {
            if (track.runtime != nullptr && track.runtime->hasLoadedInstrument())
                (void)track.runtime->enqueueMidi(message);
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
    if (track.runtime == nullptr || !track.runtime->instrumentTrack ||
        !track.runtime->hasLoadedInstrument()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    if (!track.runtime->enqueueMidi(message)) {
        error = "The target Instrument Track could not queue MIDI.";
        return false;
    }
    if (track.runtime->armed &&
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
    if (track.runtime == nullptr || !track.runtime->instrumentTrack ||
        !track.runtime->hasLoadedInstrument()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    track.runtime->panic();
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
    if (track.runtime != nullptr && track.runtime->instrument() != nullptr &&
        deviceId == track.instrumentDeviceId)
        return track.runtime->instrument()->vst3Rack();
    return track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
}

juce::var TimelineEngine::deviceStatus(const juce::String& trackId, const juce::String& deviceId,
                                       juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin status."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->status();
    if (!result.isObject()) {
        error = "Plugin status was not an object.";
        return {};
    }
    auto* object = result.getDynamicObject();
    object->setProperty("type", "trackDeviceStatus");
    object->setProperty("id", deviceId);
    object->setProperty("source", "vst3");
    object->setProperty("parameterCount", static_cast<int>(rack->parameterCount()));
    object->setProperty("statePersisted", true);
    auto* capabilities = new juce::DynamicObject();
    capabilities->setProperty("parameters", rack->parameterCount() > 0);
    capabilities->setProperty("state", true);
    capabilities->setProperty("presets", rack->hasPrograms());
    capabilities->setProperty("editor", rack->hasEditor());
    object->setProperty("capabilities", juce::var(capabilities));
    return result;
}

juce::var TimelineEngine::deviceParameterStatus(const juce::String& trackId,
                                                const juce::String& deviceId,
                                                juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin parameters."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->parameterStatus();
    if (!result.isObject()) {
        error = "Plugin parameter status was not an object.";
        return {};
    }
    result.getDynamicObject()->setProperty("type", "trackDeviceParameters");
    return result;
}

juce::var TimelineEngine::deviceProgramStatus(const juce::String& trackId,
                                              const juce::String& deviceId,
                                              juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin programs."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->programStatus();
    if (!result.isObject()) {
        error = "Plugin program status was not an object.";
        return {};
    }
    result.getDynamicObject()->setProperty("type", "trackDevicePrograms");
    return result;
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
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                               ? track.runtime->instrument()->vst3Rack()
                               : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not provide a VST3 editor state.";
            return false;
        }
        return instrument->applyPersistedState(persistedState, error);
    }
    auto* effect =
        track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
    if (effect == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    return effect->applyPersistedState(persistedState, error);
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
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                               ? track.runtime->instrument()->vst3Rack()
                               : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not expose editable parameters.";
            return false;
        }
        instrument->enqueueParameterChange(parameterIndex, value);
        sequence.fetch_add(1, std::memory_order_relaxed);
        return true;
    }
    auto* effect =
        track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
    if (effect == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    effect->enqueueParameterChange(parameterIndex, value);
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
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        const auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                                     ? track.runtime->instrument()->vst3Rack()
                                     : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not provide persisted VST3 state.";
            return {};
        }
        return instrument->persistedState(error);
    }
    return track.runtime != nullptr ? track.runtime->effects().persistedState(deviceId, error)
                                    : juce::var();
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
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime == nullptr || track.runtime->instrument() == nullptr) {
            error = "Instrument runtime is not loaded.";
            return false;
        }
        track.runtime->instrument()->setBypassed(bypassed);
    } else {
        auto* effect =
            track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
        if (effect == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        effect->setBypassed(bypassed);
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
    const auto isInstrumentDevice = track.runtime != nullptr && track.runtime->instrumentTrack &&
                                    track.instrumentDeviceId == deviceId;
    auto* playback =
        isInstrumentDevice && track.runtime != nullptr && track.runtime->instrument() != nullptr
            ? track.runtime->instrument()->vst3Rack()
            : (isInstrumentDevice
                   ? nullptr
                   : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId)
                                               : nullptr));
    if (playback == nullptr) {
        error = isInstrumentDevice ? "Built-in instruments do not expose editable parameters."
                                   : "Track Device was not found.";
        return false;
    }
    const auto parameterStatus = playback->parameterStatus().getProperty("parameters", {});
    if (!parameterStatus.isArray() || parameterIndex < 0 ||
        parameterIndex >= parameterStatus.size()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    if (!playback->setParameter(parameterIndex, value, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDevicePersistedState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const juce::var& persistedState, juce::String& error) {
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
    PluginRack* target = nullptr;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime != nullptr && track.runtime->instrument() != nullptr)
            target = track.runtime->instrument()->vst3Rack();
    } else {
        if (track.runtime != nullptr) target = track.runtime->effects().findDevice(deviceId);
    }
    if (target == nullptr) {
        error = track.runtime != nullptr && track.runtime->instrumentTrack &&
                        track.instrumentDeviceId == deviceId
                    ? "Built-in instruments do not expose plugin state."
                    : "Track Device was not found.";
        return false;
    }
    if (!target->applyPersistedState(persistedState, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceProgram(const juce::String& trackId, const juce::String& deviceId,
                                      const int programIndex, juce::String& error) {
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
    PluginRack* target = nullptr;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime != nullptr && track.runtime->instrument() != nullptr)
            target = track.runtime->instrument()->vst3Rack();
    } else {
        if (track.runtime != nullptr) target = track.runtime->effects().findDevice(deviceId);
    }
    if (target == nullptr) {
        error = track.runtime != nullptr && track.runtime->instrumentTrack &&
                        track.instrumentDeviceId == deviceId
                    ? "Built-in instruments do not expose plugin programs."
                    : "Track Device was not found.";
        return false;
    }
    const auto status = target->programStatus();
    if (!status.isObject()) {
        error = "Plugin program status was not an object.";
        return false;
    }
    const auto enumerationError = status.getProperty("error", {});
    if (!enumerationError.isVoid()) {
        error = enumerationError.toString();
        return false;
    }
    const auto programs = status.getProperty("programs", {});
    if (!programs.isArray() || programIndex < 0 || programIndex >= programs.size()) {
        error = "Plugin program index is out of range.";
        return false;
    }
    if (!target->setProgram(programIndex, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return monitorLiveInput.load(std::memory_order_acquire);
}

bool TimelineEngine::isLiveMidiTarget(const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return liveMidiTargetTrackId == trackId;
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
    auto& runtime = *track.runtime;
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : track.clips) {
        auto& clip = *clipPtr;
        if (clip.muted) continue;
        const auto clipEnd = clip.startSample + clip.durationSamples;
        const auto overlapStart = std::max(rangeStart, clip.startSample);
        const auto overlapEnd = std::min(rangeEnd, clipEnd);
        if (overlapEnd <= overlapStart) continue;
        auto& destinationBuffer = clip.processingStage == ProcessingStage::PostEffects
                                      ? runtime.postEffectClipBuffer
                                      : runtime.mixBuffer;
        auto remaining = static_cast<int>(overlapEnd - overlapStart);
        auto outputOffset = destinationStart + static_cast<int>(overlapStart - rangeStart);
        auto localSample = overlapStart - clip.startSample;
        while (remaining > 0) {
            const auto sourceRange = clip.sourceEndFrame - clip.sourceStartFrame;
            auto sourceOffset = static_cast<std::int64_t>(
                std::floor(static_cast<double>(localSample) * clip.sourceSampleRate /
                           runtime.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd =
                static_cast<int>(std::ceil(static_cast<double>(sourceRemaining) *
                                           runtime.outputSampleRate / clip.sourceSampleRate));
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
                const auto source = clip.scratch.getSample(0, sample) * envelope;
                destinationBuffer.addSample(0, outputOffset + sample, source * clip.leftGain);
                destinationBuffer.addSample(
                    1, outputOffset + sample,
                    clip.scratch.getNumChannels() > 1
                        ? clip.scratch.getSample(1, sample) * envelope * clip.rightGain
                        : source * clip.rightGain);
            }
            clip.expectedSourceFrame =
                sourceFrame +
                static_cast<std::int64_t>(std::floor(
                    static_cast<double>(chunk) * clip.sourceSampleRate / runtime.outputSampleRate));
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
    juce::ignoreUnused(prepared);
    auto& runtime = *track.runtime;
    runtime.midiBuffer.clear();
    MidiScheduler::schedule(runtime.midiClips, rangeStart, sampleCount, runtime.midiBuffer);
}

void TimelineEngine::processTracks(PreparedTimeline& prepared,
                                   const float* const* physicalInputChannels,
                                   const int physicalInputChannelCount,
                                   float* const* outputChannels, const int channelCount,
                                   const std::int64_t rangeStart, const int destinationStart,
                                   const int sampleCount) noexcept {
    const auto hasSolo = prepared.hasSolo;
    processLiveAudioTracks(prepared, physicalInputChannels, physicalInputChannelCount,
                           outputChannels, channelCount, rangeStart, destinationStart, sampleCount);
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        runtime.processedBuffer.clear(0, sampleCount);
        if (!runtime.instrumentTrack) mergeTimelineAndLiveInput(track, sampleCount);
        const float* inputChannels[2] = {runtime.mixBuffer.getWritePointer(0),
                                         runtime.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {runtime.processedBuffer.getWritePointer(0),
                                       runtime.processedBuffer.getWritePointer(1)};
        if (runtime.instrumentTrack)
            processInstrumentTrack(prepared, track, sampleCount, &runtime.midiBuffer, rangeStart);
        else
            runtime.effects().process(inputChannels, 2, processedChannels, 2, sampleCount);
        mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart, destinationStart,
                       sampleCount);
    }
}

void TimelineEngine::processLiveAudioTracks(PreparedTimeline& prepared,
                                            const float* const* physicalInputChannels,
                                            const int physicalInputChannelCount,
                                            float* const* outputChannels, const int channelCount,
                                            const std::int64_t rangeStart,
                                            const int destinationStart, const int sampleCount,
                                            const bool renderOutput) noexcept {
    const auto hasSolo = prepared.hasSolo;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        if (runtime.instrumentTrack) continue;
        runtime.liveInputBuffer.clear(0, sampleCount);
        if ((runtime.monitorInput || runtime.armed) && runtime.audioInputChannel >= 0) {
            const auto* source = ArrangementGraph::audioInputSource(
                runtime.audioInputChannel, physicalInputChannels, physicalInputChannelCount);
            for (int channel = 0; channel < 2; ++channel) {
                auto* destination = runtime.liveInputBuffer.getWritePointer(channel);
                if (source != nullptr)
                    juce::FloatVectorOperations::copy(destination, source + destinationStart,
                                                      sampleCount);
                else
                    juce::FloatVectorOperations::clear(destination, sampleCount);
            }
            if (renderOutput && runtime.monitorInput) {
                runtime.mixBuffer.clear(0, sampleCount);
                for (int channel = 0; channel < 2; ++channel) {
                    juce::FloatVectorOperations::add(
                        runtime.mixBuffer.getWritePointer(channel),
                        runtime.liveInputBuffer.getReadPointer(channel), sampleCount);
                }
            }
            const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
            const auto captureEnd =
                captureStart + captureBlockSamples.load(std::memory_order_acquire);
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (runtime.armed) {
                auto& capture = runtime.recordingCapture;
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
                        if (!recordingCapture->endTrackCapture(track.id, capture))
                            capture.state = RecordingCaptureState::idle;
                        else
                            capture.state = RecordingCaptureState::idle;
                    }
                    if (capture.state == RecordingCaptureState::idle) {
                        (void)recordingCapture->beginTrackCapture(
                            track.id, capture, captureAudioStart, captureTimelineStart);
                    }
                    if (capture.state == RecordingCaptureState::capturing) {
                        const auto writeCount = writeEnd - writeStart;
                        const auto* rawPointer =
                            runtime.liveInputBuffer.getReadPointer(0) + localOffset;
                        recordingCapture->writeAudioTrack(track.id, rawPointer, writeCount);
                        capture.endAudioSample =
                            captureAudioStart + static_cast<std::uint64_t>(writeCount);
                        capture.endTimelineSample =
                            captureTimelineStart + static_cast<std::uint64_t>(writeCount);
                    }
                } else if (capture.state == RecordingCaptureState::capturing) {
                    if (!recordingCapture->endTrackCapture(track.id, capture))
                        capture.state = RecordingCaptureState::idle;
                    else
                        capture.state = RecordingCaptureState::idle;
                }
            }
            if (renderOutput && runtime.monitorInput) {
                runtime.processedBuffer.clear(0, sampleCount);
                runtime.effects().process(runtime.mixBuffer.getArrayOfReadPointers(), 2,
                                          runtime.processedBuffer.getArrayOfWritePointers(), 2,
                                          sampleCount);
                mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart,
                               destinationStart, sampleCount);
            }
        }
    }
}

void TimelineEngine::mergeTimelineAndLiveInput(Track& track, const int sampleCount) noexcept {
    auto& runtime = *track.runtime;
    if (sampleCount <= 0) return;
    if (!runtime.monitorInput) return;
    for (int channel = 0; channel < 2; ++channel)
        juce::FloatVectorOperations::add(runtime.mixBuffer.getWritePointer(channel),
                                         runtime.liveInputBuffer.getReadPointer(channel),
                                         sampleCount);
}

void TimelineEngine::processLiveInstrumentTracks(PreparedTimeline& prepared,
                                                 float* const* outputChannels,
                                                 const int channelCount,
                                                 const std::int64_t rangeStart,
                                                 const int sampleCount) noexcept {
    const auto hasSolo = prepared.hasSolo;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        if (!runtime.instrumentTrack) continue;
        const auto audible = !runtime.muted && (!hasSolo || runtime.solo);
        processLiveInstrumentTrack(prepared, track, sampleCount, rangeStart, false);
        mixTrackOutput(track, audible, outputChannels, channelCount, rangeStart, 0, sampleCount);
    }
}

void TimelineEngine::processInstrumentTrack(PreparedTimeline& prepared, Track& track,
                                            const int sampleCount,
                                            const juce::MidiBuffer* const timelineMidi,
                                            const std::int64_t rangeStart) noexcept {
    auto& runtime = *track.runtime;
    if (runtime.instrument() != nullptr) {
        runtime.instrument()->process(runtime.mixBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                      timelineMidi,
                                      instrumentProcessContext(prepared, rangeStart, true));
    } else {
        runtime.mixBuffer.clear(0, sampleCount);
    }
    runtime.effects().process(runtime.mixBuffer.getArrayOfReadPointers(), 2,
                              runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount);
}

void TimelineEngine::processLiveInstrumentTrack(PreparedTimeline& prepared, Track& track,
                                                const int sampleCount,
                                                const std::int64_t rangeStart,
                                                const bool playing) noexcept {
    auto& runtime = *track.runtime;
    runtime.liveInputBuffer.clear(0, sampleCount);
    if (runtime.instrument() == nullptr || !runtime.liveMidiActive()) {
        runtime.processedBuffer.clear(0, sampleCount);
        return;
    }
    runtime.instrument()->process(runtime.liveInputBuffer.getArrayOfWritePointers(), 2, sampleCount,
                                  nullptr, instrumentProcessContext(prepared, rangeStart, playing));
    runtime.effects().process(runtime.liveInputBuffer.getArrayOfReadPointers(), 2,
                              runtime.processedBuffer.getArrayOfWritePointers(), 2, sampleCount);
    runtime.markLiveMidiProcessed(sampleCount);
}

void TimelineEngine::mixTrackOutput(Track& track, const bool audible, float* const* outputChannels,
                                    const int channelCount, const std::int64_t rangeStart,
                                    const int destinationStart, const int sampleCount) noexcept {
    auto& runtime = *track.runtime;
    const auto lowLatency = runtime.lowLatencyMonitoring();
    const auto processedDelay =
        lowLatency ? std::int64_t{0} : std::max<std::int64_t>(0, runtime.compensationDelaySamples);
    const auto processedDelaySize = runtime.delayBuffer.getNumSamples();
    const auto postEffectDelay =
        lowLatency ? std::int64_t{0}
                   : std::max<std::int64_t>(0, runtime.postEffectCompensationDelaySamples);
    const auto postEffectDelaySize = runtime.postEffectDelayBuffer.getNumSamples();
    auto volumeCursor = runtime.volumeAutomation.cursorAt(rangeStart);
    auto panCursor = runtime.panAutomation.cursorAt(rangeStart);
    const auto volumeAutomated = !runtime.volumeAutomation.empty();
    const auto panAutomated = !runtime.panAutomation.empty();
    const auto fixedPanAngle = (runtime.pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
    const auto fixedGain = juce::Decibels::decibelsToGain(runtime.gainDb);
    const auto fixedLeftGain = fixedGain * std::cos(fixedPanAngle);
    const auto fixedRightGain = fixedGain * std::sin(fixedPanAngle);
    const auto blockEnd = rangeStart + sampleCount;
    int processed = 0;
    while (processed < sampleCount) {
        const auto absoluteSample = rangeStart + processed;
        const auto volumeSegment =
            volumeAutomated ? volumeCursor.segmentAt(absoluteSample, blockEnd, runtime.gainDb)
                            : AutomationRuntime::Segment{blockEnd, runtime.gainDb, runtime.gainDb};
        const auto panSegment =
            panAutomated ? panCursor.segmentAt(absoluteSample, blockEnd, runtime.pan)
                         : AutomationRuntime::Segment{blockEnd, runtime.pan, runtime.pan};
        const auto segmentEnd = std::min({blockEnd, volumeSegment.endSample, panSegment.endSample});
        const auto segmentSamples =
            static_cast<int>(std::max<std::int64_t>(1, segmentEnd - absoluteSample));

        auto currentGain = fixedGain;
        auto gainRatio = 1.0f;
        auto currentCos = std::cos(fixedPanAngle);
        auto currentSin = std::sin(fixedPanAngle);
        auto deltaCos = 1.0f;
        auto deltaSin = 0.0f;
        if (volumeAutomated) {
            const auto startGain = juce::Decibels::decibelsToGain(
                juce::jlimit(-90.0f, 24.0f, volumeSegment.startValue));
            const auto endGain =
                juce::Decibels::decibelsToGain(juce::jlimit(-90.0f, 24.0f, volumeSegment.endValue));
            currentGain = startGain;
            gainRatio =
                startGain > 0.0f ? std::pow(endGain / startGain, 1.0f / segmentSamples) : 1.0f;
        }
        if (panAutomated) {
            const auto startAngle = (juce::jlimit(-1.0f, 1.0f, panSegment.startValue) + 1.0f) *
                                    juce::MathConstants<float>::pi * 0.25f;
            const auto endAngle = (juce::jlimit(-1.0f, 1.0f, panSegment.endValue) + 1.0f) *
                                  juce::MathConstants<float>::pi * 0.25f;
            currentCos = std::cos(startAngle);
            currentSin = std::sin(startAngle);
            const auto delta = (endAngle - startAngle) / segmentSamples;
            deltaCos = std::cos(delta);
            deltaSin = std::sin(delta);
        }

        for (int offset = 0; offset < segmentSamples && processed < sampleCount;
             ++offset, ++processed) {
            auto leftGain = fixedLeftGain;
            auto rightGain = fixedRightGain;
            if (volumeAutomated || panAutomated) {
                leftGain = currentGain * currentCos;
                rightGain = currentGain * currentSin;
            }
            float left = runtime.processedBuffer.getSample(0, processed);
            float right = runtime.processedBuffer.getSample(1, processed);
            if (processedDelaySize > 0) {
                const auto write = runtime.delayWritePosition;
                runtime.delayBuffer.setSample(0, static_cast<int>(write), left);
                runtime.delayBuffer.setSample(1, static_cast<int>(write), right);
                if (processedDelay > 0) {
                    const auto read =
                        (write - processedDelay + processedDelaySize) % processedDelaySize;
                    left = runtime.delayBuffer.getSample(0, static_cast<int>(read));
                    right = runtime.delayBuffer.getSample(1, static_cast<int>(read));
                }
                runtime.delayWritePosition = (write + 1) % processedDelaySize;
            }
            auto postEffectLeft = runtime.postEffectClipBuffer.getSample(0, processed);
            auto postEffectRight = runtime.postEffectClipBuffer.getSample(1, processed);
            if (postEffectDelaySize > 0) {
                const auto write = runtime.postEffectDelayWritePosition;
                runtime.postEffectDelayBuffer.setSample(0, static_cast<int>(write), postEffectLeft);
                runtime.postEffectDelayBuffer.setSample(1, static_cast<int>(write),
                                                        postEffectRight);
                if (postEffectDelay > 0) {
                    const auto read =
                        (write - postEffectDelay + postEffectDelaySize) % postEffectDelaySize;
                    postEffectLeft =
                        runtime.postEffectDelayBuffer.getSample(0, static_cast<int>(read));
                    postEffectRight =
                        runtime.postEffectDelayBuffer.getSample(1, static_cast<int>(read));
                }
                runtime.postEffectDelayWritePosition = (write + 1) % postEffectDelaySize;
            }
            left += postEffectLeft;
            right += postEffectRight;
            if (audible && channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + processed] += left * leftGain;
            if (audible && channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + processed] += right * rightGain;
            if (volumeAutomated) currentGain *= gainRatio;
            if (panAutomated) {
                const auto nextCos = currentCos * deltaCos - currentSin * deltaSin;
                currentSin = currentSin * deltaCos + currentCos * deltaSin;
                currentCos = nextCos;
            }
        }
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        auto& runtime = *track.runtime;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        runtime.mixBuffer.clear();
        runtime.processedBuffer.clear();
        runtime.postEffectClipBuffer.clear();
        runtime.midiBuffer.clear();
        runtime.resetForTransportDiscontinuity();
        runtime.delayBuffer.clear();
        runtime.delayWritePosition = 0;
        runtime.postEffectDelayBuffer.clear();
        runtime.postEffectDelayWritePosition = 0;
    }
}

void TimelineEngine::applyPendingPanic(PreparedTimeline& prepared) noexcept {
    if (!panicAllPending.exchange(false, std::memory_order_acq_rel)) return;
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.runtime != nullptr) track.runtime->panic();
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.runtime->recordingCapture);
    }
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
    if (currentState == State::stopped || currentState == State::starting) {
        for (auto& trackPtr : active->tracks)
            trackPtr->runtime->postEffectClipBuffer.clear(0, sampleCount);
        processLiveInstrumentTracks(*active, outputChannels, channelCount,
                                    timelineSample.load(std::memory_order_acquire), sampleCount);
        processLiveAudioTracks(*active, inputChannels, inputChannelCount, outputChannels,
                               channelCount, timelineSample.load(std::memory_order_acquire), 0,
                               sampleCount, true);
        return;
    }
    if (currentState != State::playing) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::stopping) {
        return;
    }
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = sampleCount - consumed;
        if (!active->tracks.empty()) {
            const auto bufferSize = active->tracks.front()->runtime->mixBuffer.getNumSamples();
            if (bufferSize > 0) chunk = std::min(chunk, bufferSize);
        }
        if (active->loopEnabled && position < active->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(active->loopEndSample - position));
        for (auto& trackPtr : active->tracks) {
            trackPtr->runtime->mixBuffer.clear(0, chunk);
            trackPtr->runtime->postEffectClipBuffer.clear(0, chunk);
        }
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
                    auto& runtime = *track.runtime;
                    if (!runtime.armed || runtime.instrumentTrack ||
                        runtime.recordingCapture.state != RecordingCaptureState::capturing)
                        continue;
                    (void)recordingCapture->endTrackCapture(track.id, runtime.recordingCapture);
                    runtime.recordingCapture.state = RecordingCaptureState::idle;
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
    object->setProperty(
        "callbackLockMisses",
        static_cast<juce::int64>(callbackLockMisses.load(std::memory_order_acquire)));
    object->setProperty(
        "callbackPublishMisses",
        static_cast<juce::int64>(callbackPublishMisses.load(std::memory_order_acquire)));
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
