#include <gtest/gtest.h>

#include "TimelineTestSupport.h"
#include "timeline/TrackProcessingPool.h"

namespace riffra {

TEST(TrackProcessingPoolTest, ProcessesEveryTrackOnceForEachWorkerCount) {
    // Arrange
    PreparedTimeline graph;
    for (int index = 0; index < 31; ++index) {
        auto track = std::make_unique<Track>();
        track->runtime = std::make_unique<TrackRuntime>();
        auto& runtime = *track->runtime;
        runtime.monitorInput = true;
        runtime.audioInputChannel = 0;
        runtime.liveInputBuffer.setSize(2, 32);
        runtime.liveInputBuffer.clear();
        runtime.processedBuffer.setSize(2, 32);
        runtime.postEffectClipBuffer.setSize(2, 32);
        runtime.postEffectClipBuffer.clear();
        runtime.trackOutputBuffer.setSize(2, 32);
        graph.processingTracks.push_back(track.get());
        graph.tracks.push_back(std::move(track));
    }
    const TrackStageJob job{TrackStageKind::liveAudioMonitor, &graph, 0, 0, 32, 1.0f, 0.0f, false};

    for (const int workerCount : {0, 1, 7}) {
        TrackProcessingPool pool(workerCount);
        for (auto& track : graph.tracks) track->runtime->windowProcessingCount = 0;

        // Act
        for (int block = 0; block < 100; ++block) pool.run(graph.processingTracks, job);

        // Assert
        for (const auto& track : graph.tracks)
            EXPECT_EQ(track->runtime->windowProcessingCount, 100u) << workerCount;
    }
}

TEST(TrackProcessingPoolTest, ReportsClosedWindowLoadsInGraphOrder) {
    // Arrange
    auto snapshot = makeTestSnapshot();
    snapshot.graph.tracks.push_back(makeInstrumentTrack("track:z"));
    snapshot.graph.tracks.push_back(makeInstrumentTrack("track:a"));
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error)) << error;
    auto* graph = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_NE(graph, nullptr);
    for (std::size_t index = 0; index < graph->tracks.size(); ++index) {
        auto& runtime = *graph->tracks[index]->runtime;
        runtime.windowProcessingTotalUs = (index + 1) * 40;
        runtime.windowProcessingCount = 2;
        runtime.windowProcessingMaximumUs = (index + 1) * 30;
    }

    // Act
    engine.closeTrackLoadWindow();
    const auto status = engine.status();

    // Assert
    ASSERT_TRUE(status.graph.has_value());
    ASSERT_EQ(status.graph->trackLoads.size(), 2u);
    EXPECT_EQ(status.graph->trackLoads[0].trackId, "track:z");
    EXPECT_EQ(status.graph->trackLoads[1].trackId, "track:a");
    EXPECT_EQ(status.graph->trackLoads[0].averageProcessingUs, 20u);
    EXPECT_EQ(status.graph->trackLoads[1].maximumProcessingUs, 60u);
    EXPECT_EQ(graph->tracks[0]->runtime->windowProcessingCount, 0u);
}

TEST(TrackProcessingPoolTest, OfflineOutputIsBitIdenticalAcrossWorkerCounts) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto audioFile = directory.get().getChildFile("source.wav");
    ASSERT_TRUE(writePcmWave(audioFile, 48'000, 1, 48'000, 4'000));
    auto snapshot = makeBuiltInInstrumentSnapshot("track:instrument");
    auto audio = makeRawAndProcessedClipSnapshot(audioFile, audioFile, 48'000).graph.tracks.front();
    audio.volumeAutomation = {{0, -12.0}, {480, -3.0}, {960, -9.0}};
    audio.panAutomation = {{0, -0.5}, {960, 0.5}};
    for (int index = 0; index < 16; ++index) {
        auto copy = audio;
        copy.id = "track:audio-" + juce::String(index);
        for (auto& clip : copy.audioClips) clip.id += "-" + juce::String(index);
        snapshot.graph.tracks.push_back(std::move(copy));
    }
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    std::array<juce::MemoryBlock, 2> rendered;

    // Act
    for (std::size_t index = 0; index < rendered.size(); ++index) {
        juce::String error;
        const auto destination =
            directory.get().getChildFile("render-" + juce::String(index) + ".wav");
        const OfflineRenderRequestSpec request{
            snapshot.graph, destination.getFullPathName(), 0, 1920, 48'000, 128, false};
        const auto caller = std::this_thread::get_id();
        std::atomic<bool> workerProcessed{false};
        std::atomic<bool> callerProcessed{false};
        std::array<ProcessorTrace, 16> traces;
        auto renderer = OfflineRenderer::prepare(request, formats, error);
        ASSERT_NE(renderer, nullptr) << error;
        TimelineEngineTestPeer::setOfflineWorkerCount(*renderer, index == 0 ? 0 : 7);
        ASSERT_TRUE(TimelineEngineTestPeer::setOfflineCompensation(*renderer, "track:audio-0", 17));
        const auto denormalsWereDisabled = juce::FloatVectorOperations::areDenormalsDisabled();
        juce::ScopedNoDenormals restoreFloatingPointState;
        juce::FloatVectorOperations::disableDenormalisedNumberSupport(false);
        for (int track = 0; track < 16; ++track) {
            traces[track].onProcess = [&] {
                if (std::this_thread::get_id() == caller) {
                    callerProcessed.store(true);
                    if (index == 1) {
                        const auto deadline =
                            std::chrono::steady_clock::now() + std::chrono::seconds(5);
                        while (!workerProcessed.load() &&
                               std::chrono::steady_clock::now() < deadline)
                            std::this_thread::yield();
                    }
                } else {
                    workerProcessed.store(true);
                }
                EXPECT_TRUE(juce::FloatVectorOperations::areDenormalsDisabled());
                volatile float subnormal = std::numeric_limits<float>::denorm_min();
                EXPECT_EQ(subnormal * 1.0e30f, 0.0f);
            };
            ASSERT_TRUE(TimelineEngineTestPeer::installOfflineEffect(
                *renderer, "track:audio-" + juce::String(track),
                std::make_unique<TestProcessor>(traces[track]), error))
                << error;
        }
        OfflineRenderer::Result result;
        const auto succeeded = renderer->render(formats, result, error);
        juce::FloatVectorOperations::disableDenormalisedNumberSupport(denormalsWereDisabled);
        ASSERT_TRUE(succeeded) << error;
        if (index == 1) EXPECT_TRUE(workerProcessed.load());
        if (index == 0) EXPECT_TRUE(callerProcessed.load());
        ASSERT_TRUE(destination.loadFileAsData(rendered[index]));
    }

    // Assert
    EXPECT_EQ(rendered[0], rendered[1]);
}

}  // namespace riffra
