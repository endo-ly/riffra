#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <vector>

#include "ArrangementGraph.h"
#include "AutomationRuntime.h"
#include "ControlGraphRegistry.h"
#include "MidiScheduler.h"
#include "MidiSourceRegistry.h"
#include "PreparedTimeline.h"
#include "RealtimeCommand.h"
#include "RealtimeFrame.h"
#include "TimelineTimebase.h"
#include "TrackRuntime.h"
#include "concurrency/BoundedMpmcQueue.h"
#include "concurrency/RealtimeCommandQueue.h"
#include "concurrency/SeqLockFrame.h"
#include "contract/ExecutionGraph.h"
#include "contract/SidecarMessages.h"
#include "instruments/InstrumentRuntime.h"
#include "plugins/PluginChain.h"
#include "recording/ArrangementCaptureSink.h"
#include "recording/RecordingCaptureRuntime.h"

namespace riffra {

class AudioRenderPipeline;
class TimelineEngineTestPeer;
class TimelineSnapshotBuilder;

/// Values derived from the active graph.
struct TimelineGraphStatus final {
    std::uint64_t revision = 0;
    double sampleRate = 0.0;
    std::uint64_t timelineTick = 0;
    std::uint64_t trackCount = 0;
    std::uint64_t instrumentRuntimeCount = 0;
    std::uint64_t pluginCount = 0;
    std::uint64_t maximumLatencySamples = 0;
    std::uint64_t liveMidiDrops = 0;
    std::vector<juce::String> armedTrackIds;
    std::vector<InstrumentFaultSpec> instrumentFaults;
};

/// Transport and graph state reported by the timeline.
struct TimelineStatus final {
    RealtimeFrame frame;
    /// Present once a graph has been published.
    std::optional<TimelineGraphStatus> graph;
};

/// Work the audio thread performs outside the timeline for one block.
struct RealtimeBlock final {
    /// Master gain of a graph published since the previous block.
    std::optional<float> publishedMasterGainDb;
};

/// Thread that currently owns the realtime timeline state.
enum class RealtimeOwner { control, audio };

/// Envelope multiplier for a normalized fade progress in [0, 1].
///
/// Applies the prepared fade shape to a normalized fade progress value.
[[nodiscard]] float fadeEnvelope(float progress, int fadeShape) noexcept;

/// Plays prepared Arrangement Graphs.
///
/// Realtime state (the active graph, transport, recording phase, seek and
/// count-in) has exactly one owner at a time: the audio thread while the
/// device runs, otherwise the control side. Control threads change it only by
/// submitting `RealtimeCommand`s and read it only through `RealtimeFrame`.
/// Graphs live in the `ControlGraphRegistry` and are destroyed on the thread
/// that constructed the engine after the audio thread retires them.
class TimelineEngine final {
public:
    using ProcessingProgressCallback = std::function<void()>;

    struct ActiveProjectMeterIdentity final {
        juce::String projectId;
        std::uint64_t meterEpoch = 0;
    };

    explicit TimelineEngine(bool offline = false);
    ~TimelineEngine();

    TimelineEngine(const TimelineEngine&) = delete;
    TimelineEngine& operator=(const TimelineEngine&) = delete;

    // Graph preparation. Message thread.
    bool loadSnapshot(const TimelineSnapshotSpec& snapshot, juce::AudioFormatManager& formats,
                      double outputSampleRate, int maximumBlockSize, juce::String& error,
                      bool commitImmediately = true);
    RealtimeRequest commitPreparedSnapshot(juce::String& error);
    void discardPreparedSnapshot() noexcept;
    [[nodiscard]] bool preparedTrackReusesRuntimeDevices(const juce::String& trackId) const;
    [[nodiscard]] bool hasPreparedSnapshot() const;
    /// Destroys the graphs the audio thread has retired. Only the thread that
    /// constructed the engine destroys graphs; other callers reclaim nothing.
    std::size_t reclaimRetiredGraphs();

    // Realtime requests. Control threads. A request that returns no sequence
    // was rejected because the realtime command queue is full.
    [[nodiscard]] std::optional<std::uint64_t> startPreparing() noexcept;
    [[nodiscard]] std::optional<std::uint64_t> play() noexcept;
    [[nodiscard]] std::optional<std::uint64_t> stop() noexcept;
    [[nodiscard]] std::optional<std::uint64_t> seekToTick(std::uint64_t tick) noexcept;
    [[nodiscard]] std::optional<std::uint64_t> panicAllInstrumentTracks() noexcept;
    RealtimeRequest startRecording(int countInBeats, juce::String& error);
    /// Cancels a running count-in; `rejected` means no count-in was running.
    RealtimeRequest cancelRecordingIfCountingIn(juce::String& error);
    /// Closes the realtime capture segments and waits until the owner has done so.
    bool stopRecording(juce::String& error);
    RealtimeRequest setLiveMidiTarget(const juce::String& trackId, juce::String& error);

    /// Switches the owner of the realtime state. The device controller calls
    /// this when the audio callback starts or has stopped.
    void setRealtimeOwner(RealtimeOwner owner);
    /// Resets clocks and track state for a newly started audio device and hands
    /// the realtime state to the audio thread.
    void audioDeviceStarted();

    /// Finalizes raw capture metadata without performing offline DSP.
    bool finalizeRecording(juce::String& error);
    /// Generates processed recording variants after the realtime graph is stopped.
    bool processFinalizedRecording(juce::String& error) noexcept;
    bool processFinalizedRecording(ArrangementCaptureSink* sink, juce::String& error,
                                   const ProcessingProgressCallback& progress = {}) noexcept;
    [[nodiscard]] juce::var recordingConfiguration() const;
    void setRecordingSink(ArrangementCaptureSink* sink) noexcept;
    void clearRecordingSink() noexcept;

    /// Control side. Returns the index live MIDI events of a device carry.
    [[nodiscard]] std::uint16_t midiSourceIndex(const juce::String& deviceId);
    /// MIDI input callback threads. Returns whether the message belongs to the
    /// timeline, which is the case whenever an armed Instrument Track listens;
    /// a message the full queue cannot take is counted in `liveMidiDrops`.
    [[nodiscard]] bool enqueueLiveMidi(std::uint16_t sourceIndex,
                                       const juce::MidiMessage& message) noexcept;
    /// Plays a Play Surface message on an Instrument Track at the next block.
    RealtimeRequest enqueueTargetedMidi(const juce::String& trackId,
                                        const juce::MidiMessage& message, juce::String& error);
    RealtimeRequest panicTargetedMidi(const juce::String& trackId, juce::String& error);

    // Committed-graph controls. Control threads.
    /// Applies a transient gain/pan change to the committed graph without changing
    /// the canonical session or rebuilding the graph.
    bool setTrackMixControl(const juce::String& trackId, std::optional<float> gainDb,
                            std::optional<float> pan, juce::String& error) noexcept;
    bool setDeviceBypassed(const juce::String& trackId, const juce::String& deviceId, bool bypassed,
                           juce::String& error) noexcept;
    bool setDeviceParameter(const juce::String& trackId, const juce::String& deviceId,
                            int parameterIndex, float value, juce::String& error) noexcept;
    bool setDeviceProgram(const juce::String& trackId, const juce::String& deviceId,
                          int programIndex, juce::String& error);
    bool setDevicePersistedState(const juce::String& trackId, const juce::String& deviceId,
                                 const PluginStateSpec& persistedState, juce::String& error);
    [[nodiscard]] std::optional<TrackDeviceStatusSpec> deviceStatus(const juce::String& trackId,
                                                                    const juce::String& deviceId,
                                                                    juce::String& error) const;
    [[nodiscard]] std::optional<TrackDeviceParametersSpec> deviceParameterStatus(
        const juce::String& trackId, const juce::String& deviceId, juce::String& error) const;
    [[nodiscard]] std::optional<TrackDeviceProgramsSpec> deviceProgramStatus(
        const juce::String& trackId, const juce::String& deviceId, juce::String& error) const;
    [[nodiscard]] PluginRack* findDevice(const juce::String& trackId,
                                         const juce::String& deviceId) noexcept;
    bool mirrorEditorDeviceState(const juce::String& trackId, const juce::String& deviceId,
                                 const PluginStateSpec& persistedState,
                                 juce::String& error) noexcept;
    bool mirrorEditorDeviceParameter(const juce::String& trackId, const juce::String& deviceId,
                                     int parameterIndex, float value, juce::String& error) noexcept;
    [[nodiscard]] std::optional<PluginStateSpec> devicePersistedState(const juce::String& trackId,
                                                                      const juce::String& deviceId,
                                                                      juce::String& error) const;

    /// Resets the processing state of every VST3 instrument and effect chain of the active
    /// graph. Plugin activation belongs to the message thread, so call this there while no
    /// block is being processed.
    void resetPluginDevices() noexcept;

    // Active-graph observation. Any non-realtime thread.
    [[nodiscard]] TimelineStatus status() const;
    /// Consumes the latest post-fader stereo meter snapshot for each active track.
    [[nodiscard]] std::vector<TrackMeterSpec> meterSnapshot();
    /// Returns the Project identity and meter epoch of the active graph.
    [[nodiscard]] ActiveProjectMeterIdentity activeProjectMeterIdentity() const;

    // Audio thread only. These methods use preallocated realtime state.
    /// Applies queued commands and opens the recording window of one block.
    [[nodiscard]] RealtimeBlock beginBlock(int sampleCount) noexcept;
    /// Silences every instrument of the active graph.
    void panicActiveGraph() noexcept;
    [[nodiscard]] std::uint64_t activeMeterEpoch() const noexcept;
    [[nodiscard]] bool monitoringEnabled() const noexcept;
    /// Returns whether the active graph routes the physical input channel to a monitored Audio
    /// Track.
    [[nodiscard]] bool monitoringInputChannel(int channel) const noexcept;
    void mixMetronome(float* const* outputChannels, int channelCount, int sampleCount) noexcept;
    void mix(float* const* outputChannels, int channelCount, int sampleCount) noexcept;
    void mix(const float* const* inputChannels, int inputChannelCount, float* const* outputChannels,
             int outputChannelCount, int sampleCount) noexcept;
    /// Processes every VST3 instrument and effect chain with silence and no MIDI, discarding the
    /// output, so plugins that finish preparing while processing can do so before playback.
    void warmUpPluginDevices(int sampleCount) noexcept;

private:
    friend class TimelineEngineTestPeer;
    friend class AudioRenderPipeline;
    friend class TimelineSnapshotBuilder;

    /// Registers the callback run when a commit crosses into another Project.
    void setProjectBoundaryCallback(std::function<void(std::uint64_t)> callback);

    using Clip = PreparedTimeline::Clip;
    using Track = PreparedTimeline::Track;

    static constexpr std::size_t kRealtimeCommandCapacity = 256;
    static constexpr std::size_t kLiveMidiCapacity = 1024;

    enum class RenderTransportState { stopped, fadingIn, playing, fadingOut };

    struct MetronomeTransportSegment final {
        std::int64_t rangeStart = 0;
        int destinationStart = 0;
        int sampleCount = 0;
        float gainStart = 0.0f;
        float gainStep = 0.0f;
        bool active = false;
    };

    /// State written only by the current realtime owner.
    struct RealtimeState final {
        PreparedTimeline* graph = nullptr;
        TransportState transport = TransportState::stopped;
        RecordingPhase recordingPhase = RecordingPhase::idle;
        std::int64_t timelineSample = 0;
        std::uint64_t audioClockSample = 0;
        std::uint64_t callbackAudioStartSample = 0;
        bool seekPending = false;
        bool seekRequestedWhileStopped = false;
        std::int64_t pendingSeekSample = 0;
        bool resetPlaybackPending = false;
        std::int64_t countInRemainingSamples = 0;
        std::int64_t countInBlockStartRemainingSamples = 0;
        int captureBlockOffset = 0;
        int captureBlockSamples = 0;
        int playbackBlockOffset = 0;
        int lastMixPlaybackOffset = 0;
        std::uint64_t recordingStartAudioSample = 0;
        std::uint64_t recordingStartTick = 0;
        std::uint32_t recordingPassOrdinal = 0;
        std::uint64_t clockGeneration = 0;
        std::uint64_t discontinuity = 1;
        std::uint64_t graphPublishCount = 0;
        std::uint64_t appliedCommandSequence = 0;
        std::uint32_t liveMidiTargetTrackKey = 0;
        /// Gain of the last published graph until a block hands it to the pipeline.
        std::optional<float> publishedMasterGainDb;
        RenderTransportState renderTransportState = RenderTransportState::stopped;
        float transportGain = 0.0f;
        float transportFadeStep = 0.0f;
        int transportFadeRemaining = 0;
        std::array<MetronomeTransportSegment, 64> metronomeTransportSegments{};
        int metronomeTransportSegmentCount = 0;
    };

    /// One live MIDI input message on its way to the audio thread.
    struct LiveMidiEvent final {
        std::uint16_t sourceIndex = 0;
        std::uint8_t bytes[3] = {};
        std::uint8_t size = 0;
    };

    struct OfflineRecordingTrack final {
        juce::String id;
        std::vector<PluginDeviceSpec> effects;
    };

    // Realtime command path.
    [[nodiscard]] std::optional<std::uint64_t> submit(RealtimeCommand command) noexcept;
    bool waitUntilApplied(std::uint64_t commandSequence, std::chrono::milliseconds timeout) const;
    void applyRealtimeCommand(RealtimeState& state, const RealtimeCommand& command) noexcept;
    void drainRealtimeCommands(RealtimeState& state) noexcept;
    void publishFrame(const RealtimeState& state) noexcept;
    void publishGraph(RealtimeState& state, PreparedTimeline* graph) noexcept;
    void advanceCountIn(RealtimeState& state, int sampleCount) noexcept;
    void applyLowLatencyMonitoring(const RealtimeState& state) noexcept;
    void startRecordingNow(RealtimeState& state, int countInBeats) noexcept;
    void routeLiveMidi(RealtimeState& state) noexcept;
    void playTargetedMidi(RealtimeState& state, const RealtimeCommand& command) noexcept;
    [[nodiscard]] static Track* findTrackByKey(const RealtimeState& state,
                                               std::uint32_t trackKey) noexcept;
    /// Validates a Play Surface request against the committed graph and returns
    /// the key of its Instrument Track.
    [[nodiscard]] std::optional<std::uint32_t> playSurfaceTrackKey(const juce::String& trackId,
                                                                   juce::String& error);
    void closeRecordingCaptures(RealtimeState& state) noexcept;
    /// Runs `visit` with the active graph under the registry lock, or returns
    /// `fallback` when no graph has been published.
    template <typename Result, typename Visit>
    Result visitActiveGraph(Result fallback, Visit&& visit) const;

    // Rendering. Audio thread (or the offline render owner).
    void mixActiveGraph(const float* const* inputChannels, int inputChannelCount,
                        float* const* outputChannels, int outputChannelCount,
                        int sampleCount) noexcept;
    void mixRange(Track& track, std::int64_t rangeStart, int destinationStart,
                  int sampleCount) noexcept;
    void processTracks(PreparedTimeline& timeline, const float* const* inputChannels,
                       int inputChannelCount, float* const* outputChannels, int channelCount,
                       std::int64_t rangeStart, int destinationStart, int sampleCount,
                       float transportGainStart, float transportGainStep) noexcept;
    void processLiveInstrumentTracks(PreparedTimeline& timeline, float* const* outputChannels,
                                     int channelCount, std::int64_t rangeStart,
                                     int destinationStart, int sampleCount) noexcept;
    void processLiveAudioTracks(PreparedTimeline& timeline, const float* const* inputChannels,
                                int inputChannelCount, float* const* outputChannels,
                                int channelCount, std::int64_t rangeStart, int destinationStart,
                                int sampleCount, bool renderOutput = false) noexcept;
    void mergeTimelineAndLiveInput(Track& track, int sampleCount) noexcept;
    void processInstrumentTrack(PreparedTimeline& timeline, Track& track, int sampleCount,
                                const juce::MidiBuffer* timelineMidi,
                                std::int64_t rangeStart) noexcept;
    void processLiveInstrumentTrack(PreparedTimeline& timeline, Track& track, int sampleCount,
                                    std::int64_t rangeStart, bool playing) noexcept;
    void mixTrackOutput(Track& track, bool audible, float* const* outputChannels, int channelCount,
                        std::int64_t rangeStart, int destinationStart, int sampleCount,
                        float transportGainStart = 1.0f, float transportGainStep = 0.0f) noexcept;
    void scheduleMidi(const PreparedTimeline& prepared, Track& track, std::int64_t rangeStart,
                      int sampleCount) noexcept;
    void resetPlaybackTrackState(PreparedTimeline& timeline) noexcept;
    void clearPlaybackTrackState(PreparedTimeline& timeline) noexcept;
    void resetRecordingTrackState(PreparedTimeline& timeline) noexcept;
    void beginTransportFade(bool fadeIn, double sampleRate) noexcept;
    void applyPendingSeek(PreparedTimeline& timeline) noexcept;
    void finishTransportFade(PreparedTimeline& timeline) noexcept;
    bool generateProcessedVariants(double sampleRate, int blockSize,
                                   const std::vector<OfflineRecordingTrack>& tracks,
                                   ArrangementCaptureSink* sink, juce::String& error,
                                   const ProcessingProgressCallback& progress) noexcept;
    [[nodiscard]] static InstrumentProcessContext instrumentProcessContext(
        const PreparedTimeline& timeline, std::int64_t rangeStart, bool playing) noexcept;
    /// Finds a plugin rack in the committed graph. The caller holds the registry lock.
    [[nodiscard]] static const PluginRack* findCommittedRack(
        const ControlGraphRegistry::State& graphs, const juce::String& trackId,
        const juce::String& deviceId, const char* noun, juce::String& error);

    juce::TimeSliceThread readAheadThread{"Riffra timeline read-ahead"};
    bool offlineMode = false;
    ControlGraphRegistry graphRegistry;
    ControlGraphRegistry::Retired retiredGraphs;
    RealtimeCommandQueue<RealtimeCommand, kRealtimeCommandCapacity> realtimeCommands;
    SeqLockFrame<RealtimeFrame> realtimeFrame;
    // Serializes command submission with ownership changes. The audio thread
    // never takes it.
    mutable std::mutex ownerMutex;
    std::atomic<RealtimeOwner> owner{RealtimeOwner::control};
    std::uint64_t nextCommandSequence = 1;
    RealtimeState realtime;
    MidiSourceRegistry midiSources;
    BoundedMpmcQueue<LiveMidiEvent, kLiveMidiCapacity> liveMidi;
    std::atomic<std::uint64_t> liveMidiQueueDrops{0};
    // Recorded as the source of Play Surface MIDI; built once so the audio
    // thread never constructs a string.
    const juce::String playSurfaceSourceId{"riffra:play-surface"};
    std::unique_ptr<RecordingCaptureRuntime> recordingCapture;
    std::mutex finalizedRecordingMutex;
    std::vector<OfflineRecordingTrack> finalizedRecordingTracks;
    double finalizedRecordingSampleRate = 0.0;
    int finalizedRecordingBlockSize = 0;
    std::function<void(std::uint64_t)> projectBoundaryCallback;
};

template <typename Result, typename Visit>
Result TimelineEngine::visitActiveGraph(Result fallback, Visit&& visit) const {
    for (;;) {
        const auto frame = realtimeFrame.read();
        if (frame.activeGraphSerial == 0) return fallback;
        std::optional<Result> result;
        graphRegistry.access([&](const ControlGraphRegistry::State& graphs) {
            if (auto* graph = graphs.find(frame.activeGraphSerial); graph != nullptr)
                result.emplace(visit(*graph, frame));
        });
        // A missing serial means the graph was retired and reclaimed after the
        // frame was read; the next frame names its successor.
        if (result.has_value()) return std::move(*result);
    }
}

}  // namespace riffra
