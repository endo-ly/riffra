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

}  // namespace riffra
