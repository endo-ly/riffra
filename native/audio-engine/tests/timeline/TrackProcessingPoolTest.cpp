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

TEST(TrackProcessingPoolTest, OfflineOutputIsBitIdenticalAcrossWorkerCounts) {
    // Arrange
    test::TemporaryDirectory directory;
    const auto audioFile = directory.get().getChildFile("source.wav");
    ASSERT_TRUE(writePcmWave(audioFile, 48'000, 1, 48'000, 4'000));
    auto snapshot = makeBuiltInInstrumentSnapshot("track:instrument");
    auto audio = makeRawAndProcessedClipSnapshot(audioFile, audioFile, 48'000).graph.tracks.front();
    audio.volumeAutomation = {{0, -12.0}, {480, -3.0}, {960, -9.0}};
    audio.panAutomation = {{0, -0.5}, {960, 0.5}};
    snapshot.graph.tracks.push_back(std::move(audio));
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
        auto renderer = OfflineRenderer::prepare(request, formats, error);
        ASSERT_NE(renderer, nullptr) << error;
        TimelineEngineTestPeer::setOfflineWorkerCount(*renderer, index == 0 ? 0 : 7);
        ASSERT_TRUE(TimelineEngineTestPeer::setOfflineCompensation(*renderer, "track:audio", 17));
        OfflineRenderer::Result result;
        ASSERT_TRUE(renderer->render(formats, result, error)) << error;
        ASSERT_TRUE(destination.loadFileAsData(rendered[index]));
    }

    // Assert
    EXPECT_EQ(rendered[0], rendered[1]);
}

}  // namespace riffra
