#include <gtest/gtest.h>

#include <optional>
#include <vector>

#include "support/TimelineTestSupport.h"
#include "timeline/TimelineSnapshotBuilder.h"

namespace riffra {

TEST(TimelineEngineTest, KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused) {
    EXPECT_TRUE(TimelineEngineTestPeer::canonicalTrackStateSurvivesReusableDeviceCommit());

    const PluginDeviceSpec original{
        "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.1f, 0.2f}, false}};
    const PluginDeviceSpec edited{
        "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.8f, 0.9f}, true}};
    EXPECT_TRUE(sameEffectTopology(std::vector<PluginDeviceSpec>{original},
                                   std::vector<PluginDeviceSpec>{edited}));
}
TEST(TimelineEngineTest, AppliesEditorEditsToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto parameter = TimelineEngineTestPeer::editorParameterUpdatesInstrumentRuntime();
    const auto state = TimelineEngineTestPeer::persistedStateUpdatesInstrumentRuntime();
    const auto program = TimelineEngineTestPeer::programChangeUpdatesInstrumentRuntime();

    // Assert: a parameter, a persisted state, and a program all reach the runtime.
    EXPECT_TRUE(parameter);
    EXPECT_TRUE(state);
    EXPECT_TRUE(program);
}
TEST(TimelineEngineTest, WarmsUpAndResetsPluginDevices) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::pluginDevicesWarmUpAndReset();

    // Assert
    EXPECT_TRUE(passed);
}
TEST(TimelineEngineTest, SendsEmergencyPanicToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::panicClosesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}
TEST(TimelineEngineTest, GraphPublicationRestoresCanonicalMasterGainAfterPreview) {
    // Arrange
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    TimelineEngine timeline;
    AudioRenderPipeline pipeline(timeline);
    pipeline.prepare(nullptr);
    auto snapshot = makeTestSnapshot();
    snapshot.graph.masterGainDb = -12.0;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(snapshot, formatManager, 48'000.0, 32, error, false))
        << error;

    // Act
    pipeline.setMasterGainDb(-3.0f);
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -3.0f);
    EXPECT_EQ(timeline.commitPreparedSnapshot(error), RealtimeRequest::accepted) << error;
    std::array<float, 32> left{};
    std::array<float, 32> right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    pipeline.processBlock(nullptr, 0, outputs.data(), 2, static_cast<int>(left.size()), {});
    pipeline.deviceStopped();

    // Assert
    EXPECT_FLOAT_EQ(pipeline.getMasterGainDb(), -12.0f);
}

TEST(TimelineDeviceTest, DroppingTheInstrumentKeepsTheSharedEffectChain) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:parked");
    snapshot.graph.tracks.front().effects.push_back(
        {"fx:parked", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}});
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    const auto* first = TimelineEngineTestPeer::committedGraph(engine);
    const auto* firstEffects = &first->tracks.front()->runtime->effects();

    // Act: a bypassed instrument holds no instance, so the parked graph
    // carries no instrument at all.
    auto parked = snapshot;
    parked.graph.tracks.front().instrument.reset();
    ASSERT_TRUE(loadTestSnapshot(engine, parked, formats, 48'000.0, 512, error)) << error;
    const auto* second = TimelineEngineTestPeer::committedGraph(engine);

    // Assert: only the instrument left; the effect chain is shared.
    EXPECT_TRUE(second->tracks.front()->reuseEffects);
    EXPECT_EQ(&second->tracks.front()->runtime->effects(), firstEffects);
    EXPECT_TRUE(second->tracks.front()->instrumentDeviceId.isEmpty());

    // Waking the instrument rebuilds only the instrument.
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    const auto* third = TimelineEngineTestPeer::committedGraph(engine);
    EXPECT_EQ(&third->tracks.front()->runtime->effects(), firstEffects);
    EXPECT_NE(third->tracks.front()->runtime->instrument(), nullptr);
}

TEST(TimelineDeviceTest, AProjectionStatesWhetherASharedDevicePlays) {
    // Arrange: bypassing mutes the resident instances before the projection
    // reaches them, so a projection that still carries the device states
    // that it plays again.
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;
    auto snapshot = makeBuiltInInstrumentSnapshot("track:bypass");
    snapshot.graph.tracks.front().effects.push_back(
        {"fx:bypass", RIFFRA_TEST_EFFECT_BUNDLE_PATH, {}});
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;
    const auto trackOwner = EffectOwnerSpec{EffectOwnerKindSpec::track, "track:bypass"};
    ASSERT_TRUE(engine.setDeviceBypassed(trackOwner, "fx:bypass", true, error)) << error;
    ASSERT_TRUE(engine.devicePersistedState(trackOwner, "fx:bypass", error)->bypassed);

    // Act: the device returned to the graph without an unbypass command.
    ASSERT_TRUE(loadTestSnapshot(engine, snapshot, formats, 48'000.0, 512, error)) << error;

    // Assert
    const auto* graph = TimelineEngineTestPeer::committedGraph(engine);
    ASSERT_TRUE(graph->tracks.front()->reuseEffects);
    EXPECT_FALSE(engine.devicePersistedState(trackOwner, "fx:bypass", error)->bypassed);
}

}  // namespace riffra
