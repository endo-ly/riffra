#include <gtest/gtest.h>

#include <array>
#include <thread>

#include "TimelineTestSupport.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSamples = 512;
// One beat at 120 BPM.
constexpr int kBeatSamples = 24'000;

TimelineSnapshotSpec revisionSnapshot(const std::uint64_t revision) {
    auto snapshot = makeAudioTrackSnapshot(1, false, true);
    snapshot.revision = revision;
    return snapshot;
}

/// Publishes a first graph, then hands the realtime state to the test's audio thread.
void startAudioOwnedTimeline(TimelineEngine& engine, juce::AudioFormatManager& formats) {
    juce::String error;
    ASSERT_TRUE(
        loadTestSnapshot(engine, revisionSnapshot(1), formats, kSampleRate, kBlockSamples, error))
        << error;
    engine.setRealtimeOwner(RealtimeOwner::audio);
}

}  // namespace

TEST(TimelineRealtimeTest, StopWinsOverACountInEndingInTheSameBlock) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    juce::String error;
    ASSERT_EQ(engine.startRecording(1, error), RealtimeRequest::accepted);
    (void)engine.beginBlock(kBeatSamples - 10);
    ASSERT_EQ(engine.status().frame.recordingPhase, RecordingPhase::countingIn);

    // Act
    ASSERT_TRUE(engine.stop());
    (void)engine.beginBlock(kBlockSamples);

    // Assert
    const auto frame = engine.status().frame;
    EXPECT_EQ(frame.transportState, TransportState::stopped);
    EXPECT_EQ(frame.recordingPhase, RecordingPhase::idle);
}

TEST(TimelineRealtimeTest, AppliesEveryCommandOfABlockInSubmissionOrder) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    ASSERT_TRUE(engine.play());
    const auto stop = engine.stop();
    ASSERT_TRUE(stop);

    // Act
    (void)engine.beginBlock(kBlockSamples);

    // Assert
    const auto frame = engine.status().frame;
    EXPECT_EQ(frame.transportState, TransportState::stopped);
    EXPECT_EQ(frame.appliedCommandSequence, *stop);
}

TEST(TimelineRealtimeTest, ReportsAPendingSeekTargetWhileStopped) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    ASSERT_TRUE(engine.seekToTick(960));

    // Act
    (void)engine.beginBlock(kBlockSamples);

    // Assert
    const auto status = engine.status();
    EXPECT_EQ(status.frame.timelineSample, kBeatSamples);
    ASSERT_TRUE(status.graph.has_value());
    EXPECT_EQ(status.graph->timelineTick, 960u);
}

TEST(TimelineRealtimeTest, DeviceStartResetsTheAudioClock) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(
        loadTestSnapshot(engine, revisionSnapshot(1), formats, kSampleRate, kBlockSamples, error));
    const auto before = engine.status().frame;

    // Act
    engine.audioDeviceStarted();

    // Assert
    const auto after = engine.status().frame;
    EXPECT_EQ(after.clockGeneration, before.clockGeneration + 1);
    EXPECT_EQ(after.audioClockSample, 0u);
    EXPECT_GT(after.discontinuity, before.discontinuity);
}

TEST(TimelineRealtimeTest, RejectsRequestsWhileTheQueueIsFull) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    for (int command = 0; command < 256; ++command) ASSERT_TRUE(engine.play());

    // Act
    const auto overflow = engine.play();
    (void)engine.beginBlock(kBlockSamples);
    const auto afterDrain = engine.play();

    // Assert
    EXPECT_FALSE(overflow);
    EXPECT_TRUE(afterDrain);
}

TEST(TimelineRealtimeTest, DestroysACommittedGraphOnlyAfterTheAudioThreadRetiresIt) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    juce::String error;
    ASSERT_TRUE(
        loadTestSnapshot(engine, revisionSnapshot(2), formats, kSampleRate, kBlockSamples, error));

    // Act
    const auto reclaimedBeforePublication = engine.reclaimRetiredGraphs();
    const auto statusBeforePublication = engine.status();
    (void)engine.beginBlock(kBlockSamples);
    const auto statusAfterPublication = engine.status();
    const auto reclaimedAfterPublication = engine.reclaimRetiredGraphs();
    const auto statusAfterReclaim = engine.status();

    // Assert
    EXPECT_EQ(reclaimedBeforePublication, 0u);
    EXPECT_EQ(reclaimedAfterPublication, 1u);
    ASSERT_TRUE(statusBeforePublication.graph.has_value());
    EXPECT_EQ(statusBeforePublication.graph->revision, 1u);
    EXPECT_EQ(statusBeforePublication.graph->sampleRate, kSampleRate);
    ASSERT_TRUE(statusAfterPublication.graph.has_value());
    EXPECT_EQ(statusAfterPublication.graph->revision, 2u);
    EXPECT_EQ(statusAfterPublication.graph->trackCount, 1u);
    ASSERT_TRUE(statusAfterReclaim.graph.has_value());
    EXPECT_EQ(statusAfterReclaim.graph->revision, 2u);
    EXPECT_EQ(statusAfterReclaim.graph->sampleRate, kSampleRate);
}

TEST(TimelineRealtimeTest, OnlyTheConstructingThreadDestroysGraphs) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    juce::String error;
    ASSERT_TRUE(
        loadTestSnapshot(engine, revisionSnapshot(2), formats, kSampleRate, kBlockSamples, error));
    (void)engine.beginBlock(kBlockSamples);

    // Act
    std::size_t reclaimedElsewhere = 0;
    std::thread([&] { reclaimedElsewhere = engine.reclaimRetiredGraphs(); }).join();
    const auto reclaimedHere = engine.reclaimRetiredGraphs();

    // Assert
    EXPECT_EQ(reclaimedElsewhere, 0u);
    EXPECT_EQ(reclaimedHere, 1u);
}

TEST(TimelineRealtimeTest, ReusedTracksShareTheirDeviceInstances) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    const auto snapshot = makeInstrumentSnapshot("track:shared");
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));
    const auto* first = TimelineEngineTestPeer::committedGraph(engine);
    const auto* firstEffects = &first->tracks.front()->runtime->effects();

    // Act
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));

    // Assert
    const auto* second = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_NE(second, first);
    EXPECT_TRUE(second->tracks.front()->reuseRuntimeDevices);
    EXPECT_EQ(&second->tracks.front()->runtime->effects(), firstEffects);
}

TEST(TimelineRealtimeTest, ValidatesLiveMidiTargetAgainstOnlyTheCommittedGraph) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeInstrumentSnapshot("track:committed"), formats,
                                 kSampleRate, kBlockSamples, error))
        << error;
    auto pending = makeInstrumentSnapshot("track:pending");
    ASSERT_TRUE(
        loadTestSnapshot(engine, pending, formats, kSampleRate, kBlockSamples, error, false))
        << error;

    // Act
    const auto pendingTarget = engine.setLiveMidiTarget("track:pending", error);
    const auto committedTarget = engine.setLiveMidiTarget("track:committed", error);

    // Assert
    EXPECT_EQ(pendingTarget, RealtimeRequest::rejected);
    EXPECT_EQ(committedTarget, RealtimeRequest::accepted);
}

TEST(TimelineRealtimeTest, RecordsMidiSourceIdFromThePublishedGraph) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeInstrumentSnapshot("track:live-midi-source");
    auto& track = snapshot.graph.tracks.front();
    track.armed = true;
    track.midiInput.deviceId = "midi:keyboard";
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error))
        << error;
    const auto sourceIndex = engine.midiSourceIndex("midi:keyboard");
    CaptureIsolationSink sink;
    engine.setRecordingSink(&sink);
    ASSERT_EQ(engine.startRecording(0, error), RealtimeRequest::accepted);
    engine.setRealtimeOwner(RealtimeOwner::audio);

    // Act
    ASSERT_TRUE(engine.enqueueLiveMidi(sourceIndex, juce::MidiMessage::noteOn(1, 60, 0.8f)));
    (void)engine.beginBlock(kBlockSamples);

    // Assert
    EXPECT_EQ(sink.receivedMidiSourceId, "midi:keyboard");
    engine.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_EQ(engine.stopArrangeRecording(error), RealtimeRequest::accepted);
    engine.clearRecordingSink();
}

TEST(TimelineRealtimeTest, DeliversLiveMidiToTheArmedInstrumentAtTheNextBlock) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeInstrumentSnapshot("track:live");
    snapshot.graph.tracks.front().armed = true;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));
    InstrumentTrace trace;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSamples, error);
    ASSERT_NE(rack, nullptr) << error;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:live",
                                                               "instrument:live", std::move(rack)));
    engine.setRealtimeOwner(RealtimeOwner::audio);
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    const auto routed = engine.enqueueLiveMidi(0, juce::MidiMessage::noteOn(1, 60, 0.8f));
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);

    // Assert
    EXPECT_TRUE(routed);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
}

TEST(TimelineRealtimeTest, CountsLiveMidiTheFullQueueCannotTake) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeInstrumentSnapshot("track:live");
    snapshot.graph.tracks.front().armed = true;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));
    engine.setRealtimeOwner(RealtimeOwner::audio);
    const auto note = juce::MidiMessage::noteOn(1, 60, 0.8f);
    for (int event = 0; event < 1024; ++event) ASSERT_TRUE(engine.enqueueLiveMidi(0, note));

    // Act
    const auto routed = engine.enqueueLiveMidi(0, note);

    // Assert
    EXPECT_TRUE(routed);
    const auto status = engine.status();
    ASSERT_TRUE(status.graph.has_value());
    EXPECT_EQ(status.graph->liveMidiDrops, 1u);
}

}  // namespace riffra
