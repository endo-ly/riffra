#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, CoversTimelinePlaybackRecordingAndRender) {
    test::TemporaryDirectory directory;
    const auto result = TimelineEngineTestPeer::run(directory.get());
    ASSERT_TRUE(result.isObject());

    const auto checks = result.getProperty("checks", {});
    ASSERT_TRUE(checks.isArray());
    for (const auto& check : *checks.getArray()) {
        const auto name = check.getProperty("name", {}).toString();
        EXPECT_TRUE(static_cast<bool>(check.getProperty("passed", false))) << name.toStdString();
    }
    EXPECT_TRUE(static_cast<bool>(result.getProperty("passed", false)));
}

TEST(TimelineEngineTest, RebuildsTimelineForTheCurrentAudioDeviceFormat) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::audioDeviceRestartRebuildsRuntimeFormat();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, ReclaimsRetiredGraphsAfterAudioReadersLeave) {
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine engine;
    juce::String error;

    ASSERT_TRUE(
        engine.loadSnapshot(makeInstrumentSnapshot("track:first"), formats, 48'000.0, 32, error))
        << error.toStdString();
    TimelineEngineTestPeer::beginAudioReadForTest(engine);

    ASSERT_TRUE(
        engine.loadSnapshot(makeInstrumentSnapshot("track:second"), formats, 48'000.0, 32, error))
        << error.toStdString();
    EXPECT_EQ(TimelineEngineTestPeer::retiredTimelineCount(engine), 1u);

    TimelineEngineTestPeer::endAudioReadForTest(engine);
    engine.serviceDeferredCleanup();

    EXPECT_EQ(TimelineEngineTestPeer::retiredTimelineCount(engine), 0u);
}

}  // namespace riffra

