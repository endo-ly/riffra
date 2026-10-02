#include "TimelineEngine.h"

#include <algorithm>
#include <cmath>
#include <thread>
#include <utility>

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

TimelineEngine::TimelineEngine(const bool offline)
    : offlineMode(offline),
      pool(std::make_unique<TrackProcessingPool>(
          std::clamp(static_cast<int>(std::thread::hardware_concurrency()) - 2, 0, 7))),
      recordingCapture(std::make_unique<RecordingCaptureRuntime>()) {
    if (!offlineMode) readAheadThread.startThread();
    publishFrame(realtime);
}

TimelineEngine::~TimelineEngine() {
    // The audio callback has stopped, so no thread references a graph.
    realtime.graph = nullptr;
    graphRegistry.access([](ControlGraphRegistry::State& graphs) {
        graphs.pending.reset();
        graphs.latestCommitted = nullptr;
        graphs.committed.clear();
    });
    if (readAheadThread.isThreadRunning()) readAheadThread.stopThread(3000);
}

void TimelineEngine::closeTrackLoadWindow() noexcept {
    if (realtime.graph == nullptr) return;
    for (const auto& track : realtime.graph->tracks) {
        auto& runtime = *track->runtime;
        const auto average = runtime.windowProcessingCount == 0
                                 ? 0
                                 : runtime.windowProcessingTotalUs / runtime.windowProcessingCount;
        runtime.windowAverageUs.store(static_cast<std::uint32_t>(average),
                                      std::memory_order_release);
        runtime.windowMaximumUs.store(static_cast<std::uint32_t>(runtime.windowProcessingMaximumUs),
                                      std::memory_order_release);
        runtime.windowProcessingTotalUs = 0;
        runtime.windowProcessingMaximumUs = 0;
        runtime.windowProcessingCount = 0;
    }
}

void TimelineEngine::setProjectBoundaryCallback(std::function<void(std::uint64_t)> callback) {
    graphRegistry.access([this, &callback](ControlGraphRegistry::State&) {
        projectBoundaryCallback = std::move(callback);
    });
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
    TimelineSnapshotBuilder builder(*this);
    if (!builder.build(snapshot, formats, outputSampleRate, maximumBlockSize, prepared, error))
        return false;
    graphRegistry.access(
        [&prepared](ControlGraphRegistry::State& graphs) { graphs.pending = std::move(prepared); });
    if (!commitImmediately) return true;
    return commitPreparedSnapshot(error) == RealtimeRequest::accepted;
}

RealtimeRequest TimelineEngine::commitPreparedSnapshot(juce::String& error) {
    reclaimRetiredGraphs();
    return graphRegistry.access([this, &error](ControlGraphRegistry::State& graphs) {
        if (graphs.pending == nullptr) {
            error = "No prepared Timeline snapshot is available.";
            return RealtimeRequest::rejected;
        }
        if (graphs.committed.size() >= ControlGraphRegistry::kRetireCapacity) {
            error = "Retired Arrangement Graphs are still waiting to be destroyed.";
            return RealtimeRequest::queueFull;
        }
        auto& candidate = *graphs.pending;
        const auto crossesProjectBoundary =
            graphs.latestCommitted == nullptr ||
            graphs.latestCommitted->projectId != candidate.projectId;
        candidate.meterEpoch = crossesProjectBoundary ? graphs.projectMeterEpoch + 1
                                                      : graphs.latestCommitted->meterEpoch;
        candidate.serial = graphs.nextSerial;
        RealtimeCommand publish;
        publish.kind = RealtimeCommand::Kind::publishGraph;
        publish.graph = &candidate;
        if (!submit(publish).has_value()) {
            error = "The realtime command queue is full.";
            return RealtimeRequest::queueFull;
        }
        ++graphs.nextSerial;
        if (crossesProjectBoundary) graphs.projectMeterEpoch = candidate.meterEpoch;
        graphs.devicesNeedReprepare = false;
        graphs.latestCommitted = &candidate;
        graphs.committed.push_back(std::move(graphs.pending));
        if (crossesProjectBoundary && projectBoundaryCallback != nullptr)
            projectBoundaryCallback(candidate.meterEpoch);
        return RealtimeRequest::accepted;
    });
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    std::unique_ptr<PreparedTimeline> discarded;
    graphRegistry.access([&discarded](ControlGraphRegistry::State& graphs) {
        discarded = std::move(graphs.pending);
    });
}

bool TimelineEngine::preparedTrackReusesRuntimeDevices(const juce::String& trackId) const {
    return graphRegistry.access([&trackId](const ControlGraphRegistry::State& graphs) {
        const auto* track =
            graphs.pending != nullptr ? graphs.pending->findTrack(trackId) : nullptr;
        return track != nullptr && track->reuseRuntimeDevices;
    });
}

bool TimelineEngine::hasPreparedSnapshot() const {
    return graphRegistry.access(
        [](const ControlGraphRegistry::State& graphs) { return graphs.pending != nullptr; });
}

std::size_t TimelineEngine::reclaimRetiredGraphs() { return graphRegistry.reclaim(retiredGraphs); }

std::optional<std::uint64_t> TimelineEngine::submit(RealtimeCommand command) noexcept {
    const std::lock_guard lock(ownerMutex);
    command.commandSequence = nextCommandSequence;
    if (owner.load(std::memory_order_relaxed) == RealtimeOwner::control) {
        applyRealtimeCommand(realtime, command);
        publishFrame(realtime);
    } else if (!realtimeCommands.tryPush(command)) {
        return std::nullopt;
    }
    return nextCommandSequence++;
}

bool TimelineEngine::waitUntilApplied(const std::uint64_t commandSequence,
                                      const std::chrono::milliseconds timeout) const {
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    while (realtimeFrame.read().appliedCommandSequence < commandSequence) {
        if (std::chrono::steady_clock::now() >= deadline) return false;
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    return true;
}

bool TimelineEngine::waitForCommandApplied(const std::uint64_t commandSequence,
                                           const std::chrono::milliseconds timeout) const {
    return waitUntilApplied(commandSequence, timeout);
}

void TimelineEngine::waitForCommandApplied(const std::uint64_t commandSequence) const {
    waitUntilApplied(commandSequence);
}

void TimelineEngine::waitUntilApplied(const std::uint64_t commandSequence) const {
    while (realtimeFrame.read().appliedCommandSequence < commandSequence)
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
}

void TimelineEngine::setRealtimeOwner(const RealtimeOwner next) {
    const std::lock_guard lock(ownerMutex);
    if (next == RealtimeOwner::control) {
        drainRealtimeCommands(realtime);
        publishFrame(realtime);
    }
    owner.store(next, std::memory_order_release);
}

void TimelineEngine::audioDeviceStarted() {
    graphRegistry.access(
        [](ControlGraphRegistry::State& graphs) { graphs.devicesNeedReprepare = true; });
    // Input played while no device ran would sound late; it is counted as dropped.
    LiveMidiEvent stale;
    while (liveMidi.tryPopNonRealtime(stale))
        liveMidiQueueDrops.fetch_add(1, std::memory_order_relaxed);
    RealtimeCommand started;
    started.kind = RealtimeCommand::Kind::deviceStarted;
    // The callback has not started yet, so the control side still owns the
    // state and applies the command immediately.
    (void)submit(started);
    setRealtimeOwner(RealtimeOwner::audio);
}

std::optional<std::uint64_t> TimelineEngine::startPreparing() noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::setStarting;
    return submit(command);
}

std::optional<std::uint64_t> TimelineEngine::play() noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::play;
    return submit(command);
}

std::optional<std::uint64_t> TimelineEngine::stop() noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::stop;
    return submit(command);
}

std::optional<std::uint64_t> TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::seek;
    command.tick = tick;
    return submit(command);
}

std::optional<std::uint64_t> TimelineEngine::panicAllInstrumentTracks() noexcept {
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::panicAll;
    return submit(command);
}

void TimelineEngine::drainRealtimeCommands(RealtimeState& state) noexcept {
    realtimeCommands.drain(
        [this, &state](const RealtimeCommand& command) { applyRealtimeCommand(state, command); });
}

void TimelineEngine::applyRealtimeCommand(RealtimeState& state,
                                          const RealtimeCommand& command) noexcept {
    switch (command.kind) {
        case RealtimeCommand::Kind::play:
            state.transport = TransportState::playing;
            break;
        case RealtimeCommand::Kind::setStarting:
            state.transport = TransportState::starting;
            break;
        case RealtimeCommand::Kind::stop:
            state.transport = TransportState::stopped;
            state.recordingPhase = RecordingPhase::idle;
            break;
        case RealtimeCommand::Kind::seek:
            if (state.graph == nullptr) break;
            state.pendingSeekSample =
                state.graph->timebase.tickToSample(command.tick, state.graph->outputSampleRate);
            state.seekPending = true;
            state.seekRequestedWhileStopped = state.transport != TransportState::playing;
            ++state.discontinuity;
            break;
        case RealtimeCommand::Kind::startRecording:
            startRecordingNow(state, command.countInBeats);
            break;
        case RealtimeCommand::Kind::stopRecording:
            closeRecordingCaptures(state);
            break;
        case RealtimeCommand::Kind::stopArrangeRecording:
            if (state.recordingPhase == RecordingPhase::countingIn) {
                state.countInRemainingSamples = 0;
                state.countInBlockStartRemainingSamples = 0;
                state.captureBlockOffset = 0;
                state.captureBlockSamples = 0;
                state.playbackBlockOffset = 0;
            } else {
                closeRecordingCaptures(state);
            }
            state.transport = TransportState::stopped;
            state.recordingPhase = RecordingPhase::idle;
            break;
        case RealtimeCommand::Kind::panicAll:
            if (state.graph != nullptr)
                for (auto& track : state.graph->tracks) track->runtime->panic();
            break;
        case RealtimeCommand::Kind::panicTrack:
            if (auto* track = findTrackByKey(state, command.trackKey); track != nullptr)
                track->runtime->panic();
            break;
        case RealtimeCommand::Kind::targetedMidi:
            playTargetedMidi(state, command);
            break;
        case RealtimeCommand::Kind::setLiveMidiTarget:
            state.liveMidiTargetTrackKey = command.trackKey;
            applyLowLatencyMonitoring(state);
            break;
        case RealtimeCommand::Kind::publishGraph:
            publishGraph(state, command.graph);
            break;
        case RealtimeCommand::Kind::setRecordingSink:
            recordingCapture->setSink(command.recordingSink);
            break;
        case RealtimeCommand::Kind::clearRecordingSink:
            recordingCapture->clearSink();
            break;
        case RealtimeCommand::Kind::deviceStarted:
            state.audioClockSample = 0;
            state.resetPlaybackPending = true;
            if (state.graph != nullptr)
                for (auto& track : state.graph->tracks) {
                    auto& runtime = *track->runtime;
                    runtime.requestTransportDiscontinuity();
                    runtime.windowProcessingTotalUs = 0;
                    runtime.windowProcessingMaximumUs = 0;
                    runtime.windowProcessingCount = 0;
                    runtime.windowAverageUs.store(0, std::memory_order_release);
                    runtime.windowMaximumUs.store(0, std::memory_order_release);
                }
            ++state.clockGeneration;
            ++state.discontinuity;
            break;
    }
    state.appliedCommandSequence = command.commandSequence;
}

void TimelineEngine::publishGraph(RealtimeState& state, PreparedTimeline* const graph) noexcept {
    auto* const previous = std::exchange(state.graph, graph);
    if (previous == nullptr)
        state.timelineSample = 0;
    else if (!retiredGraphs.retire(previous))
        jassertfalse;  // The registry never commits more graphs than the queue holds.
    state.publishedMasterGainDb = graph->masterGainDb;
    ++state.discontinuity;
    ++state.graphPublishCount;
    applyLowLatencyMonitoring(state);
}

void TimelineEngine::applyLowLatencyMonitoring(const RealtimeState& state) noexcept {
    if (state.graph == nullptr) return;
    for (auto& track : state.graph->tracks) {
        auto& runtime = *track->runtime;
        runtime.setLowLatencyMonitoring(
            runtime.instrumentTrack ? (runtime.armed || runtime.key == state.liveMidiTargetTrackKey)
                                    : runtime.monitorInput);
    }
}

void TimelineEngine::publishFrame(const RealtimeState& state) noexcept {
    RealtimeFrame frame;
    frame.appliedCommandSequence = state.appliedCommandSequence;
    frame.transportState = state.transport;
    frame.recordingPhase = state.recordingPhase;
    frame.timelineSample = state.seekPending ? state.pendingSeekSample : state.timelineSample;
    frame.audioClockSample = state.audioClockSample;
    frame.recordingStartTick = state.recordingStartTick;
    frame.recordingPassOrdinal = state.recordingPassOrdinal;
    frame.clockGeneration = state.clockGeneration;
    frame.discontinuity = state.discontinuity;
    frame.graphPublishCount = state.graphPublishCount;
    frame.liveMidiTargetTrackKey = state.liveMidiTargetTrackKey;
    if (state.graph != nullptr) {
        frame.activeGraphSerial = state.graph->serial;
        frame.graphRevision = state.graph->revision;
        frame.sampleRate = state.graph->outputSampleRate;
        frame.armedInstrumentTrack = state.graph->summary.armedInstrumentTrack;
    }
    realtimeFrame.write(frame);
}

RealtimeBlock TimelineEngine::beginBlock(const int sampleCount) noexcept {
    // Commands submitted while the control side owned the state were already
    // applied; the acquire load orders this block after that hand-over.
    if (owner.load(std::memory_order_acquire) == RealtimeOwner::audio)
        drainRealtimeCommands(realtime);
    routeLiveMidi(realtime);
    advanceCountIn(realtime, sampleCount);
    publishFrame(realtime);
    // A graph published while the device was stopped hands its gain to the
    // first block that runs.
    return {std::exchange(realtime.publishedMasterGainDb, std::nullopt)};
}

void TimelineEngine::panicActiveGraph() noexcept {
    if (realtime.graph == nullptr) return;
    for (auto& track : realtime.graph->tracks) track->runtime->panic();
}

std::uint64_t TimelineEngine::activeMeterEpoch() const noexcept {
    return realtime.graph != nullptr ? realtime.graph->meterEpoch : 0;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return realtime.graph != nullptr && realtime.graph->summary.monitorLiveInput;
}

bool TimelineEngine::monitoringInputChannel(const int channel) const noexcept {
    if (realtime.graph == nullptr || channel < 0 || channel >= 32) return false;
    const auto channels = realtime.graph->summary.monitoringInputChannels;
    return (channels & (std::uint32_t{1} << static_cast<unsigned>(channel))) != 0;
}

TimelineStatus TimelineEngine::status() const {
    TimelineStatus status;
    status.frame = realtimeFrame.read();
    status.graph = visitActiveGraph(
        std::optional<TimelineGraphStatus>{},
        [this, &status](const PreparedTimeline& graph, const RealtimeFrame& frame) {
            status.frame = frame;
            TimelineGraphStatus result;
            result.revision = graph.revision;
            result.sampleRate = graph.outputSampleRate;
            result.timelineTick =
                graph.timebase.sampleToTick(frame.timelineSample, graph.outputSampleRate);
            result.trackCount = graph.summary.trackCount;
            result.instrumentRuntimeCount = graph.summary.instrumentRuntimeCount;
            result.pluginCount = graph.summary.pluginCount;
            result.maximumLatencySamples = graph.summary.maximumLatencySamples;
            result.armedTrackIds = graph.summary.armedTrackIds;
            result.liveMidiDrops = liveMidiQueueDrops.load(std::memory_order_relaxed);
            for (const auto& track : graph.tracks) {
                result.trackLoads.push_back(
                    {track->id, track->runtime->windowAverageUs.load(std::memory_order_acquire),
                     track->runtime->windowMaximumUs.load(std::memory_order_acquire)});
                const auto* instrument = track->runtime->instrument();
                if (instrument == nullptr) continue;
                result.liveMidiDrops += instrument->droppedMidiEvents();
                result.instrumentFaults.push_back({track->id, instrument->typeName(),
                                                   instrument->faultCode(),
                                                   instrument->droppedMidiEvents()});
            }
            return std::optional<TimelineGraphStatus>{std::move(result)};
        });
    return status;
}

std::vector<TrackMeterSpec> TimelineEngine::meterSnapshot() {
    return visitActiveGraph(
        std::vector<TrackMeterSpec>{}, [](const PreparedTimeline& graph, const RealtimeFrame&) {
            std::vector<TrackMeterSpec> meters;
            meters.reserve(graph.tracks.size());
            for (const auto& track : graph.tracks) {
                const auto snapshot = track->runtime->meter.consume();
                meters.push_back({track->id, snapshot.peakLeft, snapshot.peakRight,
                                  snapshot.rmsLeft, snapshot.rmsRight});
            }
            return meters;
        });
}

TimelineEngine::ActiveProjectMeterIdentity TimelineEngine::activeProjectMeterIdentity() const {
    return visitActiveGraph(
        ActiveProjectMeterIdentity{}, [](const PreparedTimeline& graph, const RealtimeFrame&) {
            return ActiveProjectMeterIdentity{graph.projectId, graph.meterEpoch};
        });
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
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "The active Timeline graph is unavailable.";
            return false;
        }
        auto* track = graphs.latestCommitted->findTrack(trackId);
        if (track == nullptr) {
            error = "The requested Track is not present in the active Timeline graph.";
            return false;
        }
        auto& runtime = *track->runtime;
        if (gainDb.has_value())
            runtime.gainDb.store(juce::jlimit(-90.0f, 24.0f, *gainDb), std::memory_order_release);
        if (pan.has_value())
            runtime.pan.store(juce::jlimit(-1.0f, 1.0f, *pan), std::memory_order_release);
        return true;
    });
}

}  // namespace riffra
