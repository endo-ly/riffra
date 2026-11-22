#include <gtest/gtest.h>

#include <chrono>
#include <thread>

#include "support/TimelineTestSupport.h"

namespace riffra {
namespace {

struct ClockedRender final {
    bool rendered = false;
    juce::String error;
    OfflineRenderer::Result result;
    std::chrono::steady_clock::duration elapsed{};
};

ClockedRender renderRange(const ExecutionGraph& graph, juce::AudioFormatManager& formats,
                          const juce::File& destination, const std::uint64_t startTick,
                          const std::uint64_t endTick, const RenderMode renderMode) {
    OfflineRenderRequestSpec request;
    request.graph = graph;
    request.destination = destination.getFullPathName();
    request.startTick = startTick;
    request.endTick = endTick;
    request.sampleRate = 48'000;
    request.blockSize = 512;
    request.renderMode = renderMode;
    ClockedRender rendered;
    const auto renderer = OfflineRenderer::prepare(request, formats, rendered.error);
    if (renderer == nullptr) return rendered;
    const auto started = std::chrono::steady_clock::now();
    rendered.rendered = renderer->render(formats, rendered.result, rendered.error);
    rendered.elapsed = std::chrono::steady_clock::now() - started;
    return rendered;
}

}  // namespace

TEST(OfflineRendererTest, RendersBuiltInAndAutomatedVst3InstrumentsThroughOfflineRenderer) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    for (const bool vst3 : {false, true}) {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        test::TemporaryDirectory directory;
        const auto destination = directory.get().getChildFile("built-in.wav");
        OfflineRenderer::Result result;
        juce::String error;

        // Act
        auto snapshot = makeBuiltInInstrumentSnapshot("track:offline");
        if (vst3) {
            auto& track = snapshot.graph.tracks.front();
            track.panLaw = "unityCenterStereo";
            track.instrument =
                Vst3InstrumentSpec{"instrument:vst", RIFFRA_TEST_INSTRUMENT_BUNDLE_PATH, {}};
            track.midiClips[0].notes[0].durationTicks = 960;
            snapshot.graph.automation = {{AutomationTargetKind::deviceParameter,
                                          "instrument:vst",
                                          "3165055",
                                          AutomationInterpolationSpec::step,
                                          {{0, 0.25}, {240, 0.75}}}};
        }
        const auto rendered = renderTestSnapshot(snapshot.graph, formats, destination, 0, 960,
                                                 48'000, 512, false, result, error);

        // Assert
        ASSERT_TRUE(rendered) << error.toStdString();
        ASSERT_TRUE(destination.existsAsFile());
        ASSERT_GT(result.frames, 0u);
        auto reader =
            std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
        ASSERT_NE(reader, nullptr);
        ASSERT_EQ(reader->numChannels, 2u);
        ASSERT_GT(reader->lengthInSamples, 0);
        const auto frameCount = static_cast<int>(reader->lengthInSamples);
        juce::AudioBuffer<float> output(2, frameCount);
        ASSERT_TRUE(reader->read(&output, 0, frameCount, 0, true, true));
        for (int channel = 0; channel < output.getNumChannels(); ++channel)
            for (int sample = 0; sample < output.getNumSamples(); ++sample)
                ASSERT_TRUE(std::isfinite(output.getSample(channel, sample)));
        EXPECT_GT(
            std::max(output.getMagnitude(0, 0, frameCount), output.getMagnitude(1, 0, frameCount)),
            0.0f);
        if (vst3) {
            EXPECT_NEAR(output.getSample(0, 1000), 0.0625f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 10000), 0.1875f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 6143), 0.0625f, 0.0001f);
            EXPECT_NEAR(output.getSample(0, 6144), 0.1875f, 0.0001f);
            auto invalid = snapshot.graph;
            invalid.automation.front().parameterId = "absent";
            EXPECT_TRUE(renderTestSnapshot(invalid, formats,
                                           directory.get().getChildFile("unresolved.wav"), 0, 960,
                                           48'000, 512, false, result, error))
                << error.toStdString();
            ASSERT_EQ(result.automationWarnings.size(), 1u);
            EXPECT_TRUE(result.automationWarnings.front().contains("instrument:vst/absent"));
        }
    }
}

TEST(OfflineRendererTest, OfflineRendererReturnsIdenticalFrameCountWithAndWithoutTerminalNotes) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto terminalSnapshot = makeBuiltInInstrumentSnapshot("track:terminal");
    terminalSnapshot.graph.tracks.front().midiClips.front().notes.front().durationTicks = 960;
    auto plainSnapshot = makeBuiltInInstrumentSnapshot("track:plain");
    OfflineRenderer::Result terminalResult;
    OfflineRenderer::Result plainResult;
    juce::String error;

    // Act
    const auto renderTo = [&](const ExecutionGraph& graph, const char* fileName) {
        const OfflineRenderRequestSpec request{
            graph,  directory.get().getChildFile(fileName).getFullPathName(),
            0,      960,
            48'000, 512,
            false,  0.0,
            true};
        const auto renderer = OfflineRenderer::prepare(request, formats, error);
        OfflineRenderer::Result result;
        const auto success = renderer != nullptr && renderer->render(formats, result, error);
        return std::pair(success, result);
    };
    const auto terminal = renderTo(terminalSnapshot.graph, "terminal.wav");
    const auto plain = renderTo(plainSnapshot.graph, "plain.wav");

    // Assert
    ASSERT_TRUE(terminal.first) << error.toStdString();
    ASSERT_TRUE(plain.first) << error.toStdString();
    EXPECT_EQ(terminal.second.frames, plain.second.frames);
    EXPECT_GT(terminal.second.frames, 0u);
    const auto terminalReader = std::unique_ptr<juce::AudioFormatReader>(
        formats.createReaderFor(directory.get().getChildFile("terminal.wav")));
    ASSERT_NE(terminalReader, nullptr);
    EXPECT_EQ(static_cast<std::uint64_t>(terminalReader->lengthInSamples), terminal.second.frames);
}

TEST(OfflineRendererTest, OfflineRendererReturnsIdenticalFrameCountWithAndWithoutTrackLatency) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto effectSnapshot = makeBuiltInInstrumentSnapshot("track:latency-effect");
    effectSnapshot.graph.tracks.front().effects.push_back(
        {"fx:offline", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}});
    OfflineRenderer::Result plainResult;
    OfflineRenderer::Result effectResult;
    juce::String error;

    // Act
    const auto renderTo = [&](const ExecutionGraph& graph, const char* fileName,
                              OfflineRenderer::Result& result) {
        const OfflineRenderRequestSpec request{
            graph,  directory.get().getChildFile(fileName).getFullPathName(),
            0,      960,
            48'000, 512,
            false,  0.0,
            true};
        const auto renderer = OfflineRenderer::prepare(request, formats, error);
        return renderer != nullptr && renderer->render(formats, result, error);
    };
    const auto plain = renderTo(makeBuiltInInstrumentSnapshot("track:latency-plain").graph,
                                "latency-plain.wav", plainResult);
    const auto effect = renderTo(effectSnapshot.graph, "latency-effect.wav", effectResult);

    // Assert
    ASSERT_TRUE(plain) << error.toStdString();
    ASSERT_TRUE(effect) << error.toStdString();
    EXPECT_EQ(plainResult.frames, effectResult.frames);
    EXPECT_GT(plainResult.frames, 0u);
    const auto effectReader = std::unique_ptr<juce::AudioFormatReader>(
        formats.createReaderFor(directory.get().getChildFile("latency-effect.wav")));
    ASSERT_NE(effectReader, nullptr);
    EXPECT_EQ(static_cast<std::uint64_t>(effectReader->lengthInSamples), effectResult.frames);
}

TEST(OfflineRendererTest, NormalizesAnOfflineRenderToTheTargetPeak) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:render");
    const auto destination = directory.get().getChildFile("normalized.wav");

    // Act
    OfflineRenderer::Result result;
    juce::String error;
    ASSERT_TRUE(renderTestSnapshot(snapshot.graph, formats, destination, 0, 1'920, 48'000, 512,
                                   true, result, error))
        << error;

    // Assert: the rendered output reaches the normalized target peak.
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(destination));
    ASSERT_NE(reader, nullptr);
    const auto sampleCount = static_cast<int>(reader->lengthInSamples);
    ASSERT_GT(sampleCount, 0);
    juce::AudioBuffer<float> rendered(2, sampleCount);
    ASSERT_TRUE(reader->read(&rendered, 0, sampleCount, 0, true, true));
    const auto peak = std::max(rendered.getMagnitude(0, 0, sampleCount),
                               rendered.getMagnitude(1, 0, sampleCount));
    EXPECT_NEAR(peak, 0.98f, 0.02f);
}

TEST(OfflineRendererTest, OfflineRenderCompletesFasterThanRealTime) {
    // Arrange: 4 seconds of audio.
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:offline-pace");

    // Act
    const auto render =
        renderRange(snapshot.graph, formats, directory.get().getChildFile("offline.wav"), 0, 7'680,
                    RenderMode::offline);

    // Assert
    ASSERT_TRUE(render.rendered) << render.error;
    EXPECT_LT(render.elapsed, std::chrono::seconds(2));
}

TEST(OfflineRendererTest, RealtimeRenderFollowsWallClockPace) {
    // Arrange: 0.2 seconds of audio at 120 BPM.
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:realtime-pace");

    // Act
    const auto render =
        renderRange(snapshot.graph, formats, directory.get().getChildFile("realtime.wav"), 0, 384,
                    RenderMode::realtime);

    // Assert
    ASSERT_TRUE(render.rendered) << render.error;
    EXPECT_GE(render.elapsed, std::chrono::milliseconds(200));
}

TEST(OfflineRendererTest, RealtimeRenderOfALaterRangePacesFromItsFirstBlock) {
    // Arrange: the range starts 0.5 seconds into the timeline and lasts 0.2 seconds. The engine
    // processes the graph from the timeline start up to the end tick, so the whole 0.7 seconds
    // paces; the later start position is not slept a second time.
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:range-pace");

    // Act
    const auto render =
        renderRange(snapshot.graph, formats, directory.get().getChildFile("range.wav"), 960, 1'344,
                    RenderMode::realtime);

    // Assert: pacing counts processed samples, not the timeline position, so the render takes
    // about 0.7 seconds and never the extra 0.5 seconds of the start position.
    ASSERT_TRUE(render.rendered) << render.error;
    EXPECT_GE(render.elapsed, std::chrono::milliseconds(700));
    EXPECT_LT(render.elapsed, std::chrono::seconds(1));
    EXPECT_EQ(render.result.frames, 9'600u);
}

TEST(OfflineRendererTest, CancellingARealtimeRenderStopsTheWaitPromptly) {
    // Arrange: 10 seconds of realtime audio, cancelled while the first blocks are paced.
    juce::ScopedJuceInitialiser_GUI initialiser;
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    test::TemporaryDirectory directory;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:cancel");
    const auto destination = directory.get().getChildFile("cancelled.wav");
    OfflineRenderRequestSpec request;
    request.graph = snapshot.graph;
    request.destination = destination.getFullPathName();
    request.endTick = 19'200;
    request.sampleRate = 48'000;
    request.blockSize = 512;
    request.renderMode = RenderMode::realtime;
    juce::String error;
    const auto renderer = OfflineRenderer::prepare(request, formats, error);
    ASSERT_NE(renderer, nullptr) << error;
    OfflineRenderer::Result result;
    auto rendered = true;
    std::chrono::steady_clock::time_point cancelledAt;

    // Act
    std::thread renderThread([&] { rendered = renderer->render(formats, result, error); });
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    cancelledAt = std::chrono::steady_clock::now();
    renderer->cancel();
    renderThread.join();

    // Assert: the wait observes the request instead of running to the end of the range.
    EXPECT_FALSE(rendered);
    EXPECT_TRUE(error.containsIgnoreCase("cancelled")) << error;
    EXPECT_LT(std::chrono::steady_clock::now() - cancelledAt, std::chrono::milliseconds(100));
    EXPECT_FALSE(destination.existsAsFile());
    EXPECT_FALSE(destination.getSiblingFile("cancelled.wav.partial").existsAsFile());
}

}  // namespace riffra
