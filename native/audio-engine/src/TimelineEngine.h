#pragma once

#include <JuceHeader.h>
#include "ArrangementCaptureSink.h"
#include "ArrangementGraph.h"
#include "PluginChain.h"
#include "RecordingCaptureRuntime.h"
#include "TimelineTimebase.h"

#include <atomic>
#include <cstdint>
#include <memory>
#include <vector>

namespace riffra {

class TimelineEngine final {
public:
    explicit TimelineEngine(bool offline = false);
    ~TimelineEngine();

    TimelineEngine(const TimelineEngine&) = delete;
    TimelineEngine& operator=(const TimelineEngine&) = delete;

    bool loadSnapshot(
        const juce::var& snapshot,
        juce::AudioFormatManager& formats,
        double outputSampleRate,
        int maximumBlockSize,
        juce::String& error,
        bool commitImmediately = true);
    bool commitPreparedSnapshot(juce::String& error) noexcept;
    void discardPreparedSnapshot() noexcept;
    void play() noexcept;
    void stop() noexcept;
    void audioDeviceStarted() noexcept;
    void seekToTick(std::uint64_t tick) noexcept;
    bool startRecording(int countInBeats, juce::String& error) noexcept;
    bool cancelRecordingIfCountingIn() noexcept;
    void stopRecording() noexcept;
    bool flushRecordingTail(juce::String& error) noexcept;
    [[nodiscard]] juce::var recordingConfiguration() const;
    void setRecordingSink(ArrangementCaptureSink* sink) noexcept;
    void clearRecordingSink() noexcept;
    [[nodiscard]] bool enqueueLiveMidi(
        const juce::MidiMessage& message,
        const juce::String& deviceId = {}) noexcept;
    bool setDeviceBypassed(
        const juce::String& trackId,
        const juce::String& deviceId,
        bool bypassed,
        juce::String& error) noexcept;
    bool setDeviceParameter(
        const juce::String& trackId,
        const juce::String& deviceId,
        int parameterIndex,
        float value,
        juce::String& error) noexcept;
    [[nodiscard]] PluginRack* findDevice(
        const juce::String& trackId,
        const juce::String& deviceId) noexcept;
    bool mirrorEditorDeviceState(
        const juce::String& trackId,
        const juce::String& deviceId,
        const juce::var& persistedState,
        juce::String& error) noexcept;
    bool mirrorEditorDeviceParameter(
        const juce::String& trackId,
        const juce::String& deviceId,
        int parameterIndex,
        float value,
        juce::String& error) noexcept;
    [[nodiscard]] juce::var devicePersistedState(
        const juce::String& trackId,
        const juce::String& deviceId,
        juce::String& error) const;
    [[nodiscard]] bool preparedTrackReusesRuntimeDevices(
        const juce::String& trackId) const noexcept;
    [[nodiscard]] bool monitoringEnabled() const noexcept;
    [[nodiscard]] bool recordingWindow(
        int sampleCount,
        int& sampleOffset,
        int& capturedSamples) noexcept;
    void mixMetronome(float* const* outputChannels, int channelCount, int sampleCount) noexcept;
    void mix(float* const* outputChannels, int channelCount, int sampleCount) noexcept;
    void mix(
        const float* const* inputChannels,
        int inputChannelCount,
        float* const* outputChannels,
        int outputChannelCount,
        int sampleCount) noexcept;
    [[nodiscard]] juce::var status() const;

private:
    friend juce::var runTimelineSelfTest(const juce::File& directory);

    enum class State { stopped, playing, faulted };
    enum class RecordingPhase { idle, countingIn, recording, stopping };

    struct Clip final {
        juce::String id;
        std::unique_ptr<juce::AudioFormatReaderSource> readerSource;
        juce::AudioTransportSource transport;
        juce::AudioBuffer<float> scratch;
        std::int64_t startSample = 0;
        std::int64_t sourceStartFrame = 0;
        std::int64_t sourceEndFrame = 0;
        std::int64_t durationSamples = 0;
        std::int64_t expectedSourceFrame = -1;
        double sourceSampleRate = 0.0;
        float gain = 1.0f;
        float pan = 0.0f;
        std::int64_t fadeInSamples = 0;
        std::int64_t fadeOutSamples = 0;
        bool loop = false;
        bool muted = false;
    };

    struct MidiNote final {
        std::uint64_t startTick = 0;
        std::uint64_t durationTicks = 1;
        int note = 0;
        int velocity = 0;
        int channel = 1;
    };

    struct MidiEvent final {
        juce::String kind;
        std::uint64_t tick = 0;
        int channel = 1;
        int data1 = 0;
        int data2 = 0;
    };

    struct MidiClip final {
        std::uint64_t startTick = 0;
        std::uint64_t durationTicks = 1;
        bool loop = false;
        bool muted = false;
        std::vector<MidiNote> notes;
        std::vector<MidiEvent> events;
    };

    struct Track final {
        juce::String id;
        std::vector<std::unique_ptr<Clip>> clips;
        std::vector<MidiClip> midiClips;
        std::unique_ptr<PluginRack> instrumentRack;
        juce::String instrumentDeviceId;
        juce::String effectTopologySignature;
        juce::String instrumentTopologySignature;
        juce::var effectState;
        juce::var instrumentState;
        bool reuseRuntimeDevices = false;
        bool effectStateChanged = false;
        bool instrumentStateChanged = false;
        PluginChain effectChain;
        PluginChain liveEffectChain;
        juce::AudioBuffer<float> mixBuffer;
        juce::AudioBuffer<float> processedBuffer;
        juce::AudioBuffer<float> liveInputBuffer;
        juce::AudioBuffer<float> liveProcessedBuffer;
        RecordingCaptureTrackState recordingCapture;
        juce::AudioBuffer<float> delayBuffer;
        std::int64_t delayWritePosition = 0;
        std::int64_t compensationDelaySamples = 0;
        std::int64_t pluginDelaySamples = 0;
        std::int64_t pluginTailSamples = 0;
        double outputSampleRate = 0.0;
        float gainDb = 0.0f;
        float pan = 0.0f;
        std::vector<ArrangementGraph::AutomationPoint> volumeAutomation;
        std::vector<ArrangementGraph::AutomationPoint> panAutomation;
        bool muted = false;
        bool solo = false;
        bool instrument = false;
        bool armed = false;
        int audioInputChannel = -1;
        bool monitorInput = false;
        juce::String midiDeviceId;
        int midiChannel = 0;
        juce::MidiBuffer midiBuffer;
    };

    struct PreparedTimeline final {
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
        std::int64_t beatSamples = 0;
        std::int64_t beatsPerBar = 4;
        juce::Array<juce::var> unavailableClipIds;
        juce::Array<juce::var> missingDeviceIds;
        std::vector<std::unique_ptr<Track>> tracks;
    };

    bool prepareSnapshot(
        const juce::var& snapshot,
        juce::AudioFormatManager& formats,
        double outputSampleRate,
        int maximumBlockSize,
        std::unique_ptr<PreparedTimeline>& prepared,
        bool& monitorLiveInputState,
        bool& armedInstrumentTrackState,
        juce::String& error);
    void mixRange(
        Track& track,
        std::int64_t rangeStart,
        int destinationStart,
        int sampleCount) noexcept;
    void processTracks(
        PreparedTimeline& timeline,
        const float* const* inputChannels,
        int inputChannelCount,
        float* const* outputChannels,
        int channelCount,
        std::int64_t rangeStart,
        int destinationStart,
        int sampleCount) noexcept;
    void scheduleMidi(
        const PreparedTimeline& prepared,
        Track& track,
        std::int64_t rangeStart,
        int sampleCount) noexcept;
    void resetPlaybackTrackState(PreparedTimeline& timeline) noexcept;
    void resetRecordingTrackState(PreparedTimeline& timeline) noexcept;
    bool generateLoopProcessedVariants(
        PreparedTimeline& timeline,
        ArrangementCaptureSink* sink) noexcept;

    juce::TimeSliceThread readAheadThread { "Riffra timeline read-ahead" };
    bool offlineMode = false;
    mutable juce::SpinLock timelineLock;
    std::unique_ptr<PreparedTimeline> timeline;
    std::unique_ptr<PreparedTimeline> pendingTimeline;
    bool pendingMonitorLiveInput = false;
    bool pendingArmedInstrumentTrack = false;
    std::unique_ptr<RecordingCaptureRuntime> recordingCapture;
    std::atomic<State> state { State::stopped };
    std::atomic<std::int64_t> timelineSample { 0 };
    std::atomic<std::int64_t> lastMixStartSample { 0 };
    std::atomic<std::uint64_t> audioClockSample { 0 };
    std::atomic<std::uint64_t> callbackAudioStartSample { 0 };
    mutable std::atomic<std::uint64_t> sequence { 0 };
    std::atomic<std::uint64_t> callbackLockMisses { 0 };
    std::atomic<std::uint64_t> clockGeneration { 0 };
    std::atomic<std::uint64_t> discontinuity { 1 };
    std::atomic<bool> monitorLiveInput { false };
    std::atomic<bool> armedInstrumentTrack { false };
    std::atomic<RecordingPhase> recordingPhase { RecordingPhase::idle };
    std::atomic<std::int64_t> countInRemainingSamples { 0 };
    std::atomic<std::int64_t> countInBlockStartRemainingSamples { 0 };
    std::atomic<std::uint64_t> recordingStartAudioSample { 0 };
    std::atomic<std::uint64_t> recordingStartTick { 0 };
    std::atomic<std::uint32_t> recordingPassOrdinal { 0 };
    std::atomic<int> captureBlockOffset { 0 };
    std::atomic<int> captureBlockSamples { 0 };
    std::atomic<int> playbackBlockOffset { 0 };
    std::atomic<int> lastMixPlaybackOffset { 0 };
};

[[nodiscard]] juce::var runTimelineSelfTest(const juce::File& directory);

} // namespace riffra
