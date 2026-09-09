#include <gtest/gtest.h>

#include "TimelineTestSupport.h"

namespace riffra {

TEST(TimelineEngineTest, KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused) {
    EXPECT_TRUE(TimelineEngineTestPeer::canonicalTrackStateSurvivesReusableDeviceCommit());
}

TEST(TimelineEngineTest, AppliesEditorParameterToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::editorParameterUpdatesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

TEST(TimelineEngineTest, AppliesPluginStateToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::persistedStateUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, AppliesPluginProgramToTheInstrumentRuntime) {
    EXPECT_TRUE(TimelineEngineTestPeer::programChangeUpdatesInstrumentRuntime());
}

TEST(TimelineEngineTest, SendsEmergencyPanicToTheInstrumentRuntime) {
    // Arrange
    // Act
    const auto passed = TimelineEngineTestPeer::panicClosesInstrumentRuntime();

    // Assert
    EXPECT_TRUE(passed);
}

}  // namespace riffra

