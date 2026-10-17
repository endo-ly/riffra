#include <gtest/gtest.h>

#include <array>
#include <future>
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

TEST(TimelineRealtimeTest, PauseFreezesThePlayheadUntilResumeAndStopRestoresTransport) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    ASSERT_TRUE(engine.play());
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    ASSERT_GT(engine.status().frame.timelineSample, 0);

    // Act
    ASSERT_TRUE(engine.pause());
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto paused = engine.status().frame;
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto held = engine.status().frame;
    ASSERT_TRUE(engine.play());
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    const auto resumed = engine.status().frame;

    // Assert
    EXPECT_EQ(paused.transportState, TransportState::paused);
    EXPECT_EQ(held.transportState, TransportState::paused);
    EXPECT_EQ(held.timelineSample, paused.timelineSample);
    EXPECT_EQ(resumed.transportState, TransportState::playing);
    EXPECT_GT(resumed.timelineSample, held.timelineSample);

    ASSERT_TRUE(engine.pause());
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    ASSERT_EQ(engine.status().frame.transportState, TransportState::paused);
    ASSERT_TRUE(engine.stop());
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    EXPECT_EQ(engine.status().frame.transportState, TransportState::stopped);
}

TEST(TimelineRealtimeTest, RejectsPauseWhileArrangeRecordingIsActive) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    startAudioOwnedTimeline(engine, formats);
    juce::String error;
    ASSERT_EQ(engine.startRecording(0, error), RealtimeRequest::accepted);
    (void)engine.beginBlock(kBlockSamples);
    ASSERT_EQ(engine.status().frame.recordingPhase, RecordingPhase::recording);

    // Act
    const auto pause = engine.pause();

    // Assert
    EXPECT_FALSE(pause.has_value());
    EXPECT_EQ(engine.status().frame.transportState, TransportState::playing);
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::recording);

    engine.setRealtimeOwner(RealtimeOwner::control);
    ASSERT_EQ(engine.stopArrangeRecording(error), RealtimeRequest::accepted);
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
    EXPECT_TRUE(second->tracks.front()->reuseEffects);
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

TEST(TimelineRealtimeTest, RecordsMidiSourceIdRegisteredAfterGraphPublication) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeInstrumentSnapshot("track:live-midi-source");
    auto& track = snapshot.graph.tracks.front();
    track.armed = true;
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
    engine.setRealtimeOwner(RealtimeOwner::control);
    EXPECT_EQ(engine.stopArrangeRecording(error), RealtimeRequest::accepted);
    ASSERT_TRUE(engine.finalizeRecording(error)) << error;
    EXPECT_EQ(sink.receivedMidiSourceId, "midi:keyboard");
    engine.clearRecordingSink();
}

TEST(TimelineRealtimeTest, ArrangeRecordingStopWaitsUntilItsQueuedCommandIsApplied) {
    // Arrange
    juce::AudioFormatManager formats;
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, makeAudioTrackSnapshot(1, false, true), formats,
                                 kSampleRate, kBlockSamples, error))
        << error;
    ASSERT_EQ(engine.startRecording(0, error), RealtimeRequest::accepted);
    engine.setRealtimeOwner(RealtimeOwner::audio);

    // Act
    const auto nextSequence = TimelineEngineTestPeer::nextCommandSequence(engine);
    auto stopped = std::async(std::launch::async, [&engine] {
        juce::String stopError;
        return engine.stopArrangeRecording(stopError);
    });
    EXPECT_EQ(stopped.wait_for(std::chrono::milliseconds(600)), std::future_status::timeout);
    EXPECT_EQ(TimelineEngineTestPeer::nextCommandSequence(engine), nextSequence + 1);
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::recording);
    engine.setRealtimeOwner(RealtimeOwner::control);

    // Assert
    EXPECT_EQ(stopped.get(), RealtimeRequest::accepted);
    EXPECT_EQ(engine.status().frame.recordingPhase, RecordingPhase::idle);
}

TEST(TimelineRealtimeTest, DeliversLiveMidiToTheArmedInstrumentAtTheNextBlock) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:live", false, true);
    auto& instrument = std::get<InternalInstrumentSpec>(*snapshot.graph.tracks.front().instrument);
    instrument.midi.transposeSemitones = 12;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));
    InstrumentTrace trace;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSamples, error);
    ASSERT_NE(rack, nullptr) << error;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(engine, "track:live",
                                                               "instrument:live", std::move(rack)));
    ASSERT_TRUE(TimelineEngineTestPeer::setPlaybackCompensationForTest(engine, "track:live",
                                                                       kBlockSamples));
    engine.setRealtimeOwner(RealtimeOwner::audio);
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act
    ASSERT_EQ(engine.status().frame.transportState, TransportState::stopped);
    const auto routed = engine.enqueueLiveMidi(0, juce::MidiMessage::noteOn(1, 60, 0.8f));
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);

    // Assert
    EXPECT_TRUE(routed);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 72);
    EXPECT_EQ(engine.status().frame.transportState, TransportState::stopped);
    ASSERT_FALSE(trace.midiSamplePositions.empty());
    EXPECT_EQ(trace.midiSamplePositions.front(), 0);
    EXPECT_GT(left.front(), 0.01f);
    EXPECT_GT(*std::max_element(left.begin(), left.end()), 0.01f);

    // A Live Note Off uses the same mapped key, even when the source note stays untransposed.
    ASSERT_TRUE(engine.enqueueLiveMidi(0, juce::MidiMessage::noteOff(1, 60)));
    (void)engine.beginBlock(kBlockSamples);
    engine.mix(outputs.data(), 2, kBlockSamples);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOff());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 72);
}

TEST(TimelineRealtimeTest, TransformsPlaySurfaceNotesWithoutTransposingKeyswitches) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:play-surface", false, true);
    auto& instrument = std::get<InternalInstrumentSpec>(*snapshot.graph.tracks.front().instrument);
    instrument.midi.transposeSemitones = 12;
    instrument.midi.keyswitches.push_back({48, "Sustain"});
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, kSampleRate, kBlockSamples, error));
    InstrumentTrace trace;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSamples, error);
    ASSERT_NE(rack, nullptr) << error;
    ASSERT_TRUE(TimelineEngineTestPeer::installTrackInstrument(
        engine, "track:play-surface", "instrument:play-surface", std::move(rack)));
    CaptureIsolationSink recordingSink;
    engine.setRecordingSink(&recordingSink);
    ASSERT_EQ(engine.startRecording(0, error), RealtimeRequest::accepted);
    engine.setRealtimeOwner(RealtimeOwner::audio);
    std::array<float, kBlockSamples> left{};
    std::array<float, kBlockSamples> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    const auto play = [&](const juce::MidiMessage& message) {
        const auto queued = engine.enqueueTargetedMidi("track:play-surface", message, error);
        if (queued != RealtimeRequest::accepted) return false;
        (void)engine.beginBlock(kBlockSamples);
        engine.mix(outputs.data(), 2, kBlockSamples);
        return true;
    };

    // Act and assert: a regular Note On and Note Off share the same mapping.
    ASSERT_TRUE(play(juce::MidiMessage::noteOn(1, 36, 0.8f))) << error;
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 48);
    ASSERT_TRUE(play(juce::MidiMessage::noteOff(1, 36))) << error;
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOff());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 48);

    // A source Keyswitch bypasses transpose on both message edges.
    ASSERT_TRUE(play(juce::MidiMessage::noteOn(1, 48, 0.8f))) << error;
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 48);
    ASSERT_TRUE(play(juce::MidiMessage::noteOff(1, 48))) << error;
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOff());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 48);

    engine.setRealtimeOwner(RealtimeOwner::control);
    ASSERT_EQ(engine.stopArrangeRecording(error), RealtimeRequest::accepted);
    ASSERT_TRUE(engine.finalizeRecording(error)) << error;
    EXPECT_EQ(recordingSink.midiMessageCount, 4u);
    EXPECT_EQ(recordingSink.midiNotes[0], 36);
    EXPECT_EQ(recordingSink.midiNotes[1], 36);
    EXPECT_EQ(recordingSink.midiNotes[2], 48);
    EXPECT_EQ(recordingSink.midiNotes[3], 48);
    EXPECT_TRUE(recordingSink.midiNoteOns[0]);
    EXPECT_FALSE(recordingSink.midiNoteOns[1]);
    EXPECT_TRUE(recordingSink.midiNoteOns[2]);
    EXPECT_FALSE(recordingSink.midiNoteOns[3]);
    engine.clearRecordingSink();
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

TEST(TimelineRealtimeTest, ProcessesALoopCaptureWithinThePreparedBlockSize) {
    // Arrange: the engine is prepared for 128-sample blocks.
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeTestSnapshot();
    snapshot.graph.loopRange = {true, 0, 960};
    auto track = makeAudioTrack("track:blocksize");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 128, error)) << error;
    test::TemporaryDirectory directory;
    LoopDataCaptureSink captureSink(directory.get());
    engine.setRecordingSink(&captureSink);

    constexpr int kTotalSamples = 24'000;
    constexpr int kEngineBlockSamples = 128;
    constexpr float kImpulseAmplitude = 0.8f;
    constexpr int kImpulsePosition = 500;
    ASSERT_TRUE(engine.seekToTick(0));
    int captureOffset = 0;
    int captureSamples = 0;
    ASSERT_TRUE(engine.startRecording(0, error) == RealtimeRequest::accepted);
    ASSERT_TRUE(TimelineEngineTestPeer::recordingWindow(engine, kTotalSamples, captureOffset,
                                                        captureSamples));
    std::array<float, kEngineBlockSamples> input{};
    std::array<float, kEngineBlockSamples> outputLeft{};
    std::array<float, kEngineBlockSamples> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    int mixed = 0;
    while (mixed < kTotalSamples) {
        input.fill(0.0f);
        if (mixed <= kImpulsePosition && kImpulsePosition < mixed + kEngineBlockSamples)
            input[static_cast<std::size_t>(kImpulsePosition - mixed)] = kImpulseAmplitude;
        engine.mix(inputs.data(), 1, outputs.data(), 2, kEngineBlockSamples);
        mixed += kEngineBlockSamples;
    }
    ASSERT_TRUE(engine.stopRecording(error));
    ASSERT_TRUE(finalizeCapturedRecording(engine, error));
    engine.clearRecordingSink();

    // Assert: the processed variant is chunked to the prepared block size and the
    // impulse survives the split at its original position.
    EXPECT_EQ(captureSink.totalRaw, kTotalSamples);
    EXPECT_EQ(captureSink.totalProcessed, kTotalSamples);
    EXPECT_GT(captureSink.offlineProcessedWriteCalls, 1);
    EXPECT_LE(captureSink.maxOfflineProcessedWriteSize, kEngineBlockSamples);
    ASSERT_LT(static_cast<std::size_t>(kImpulsePosition), captureSink.processedLeft.size());
    EXPECT_NEAR(captureSink.processedLeft[static_cast<std::size_t>(kImpulsePosition)],
                kImpulseAmplitude, 0.01f);
}

}  // namespace riffra
