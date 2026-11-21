#include <gtest/gtest.h>

#include "app/AudioStatusBuilder.h"
#include "device/MidiInputService.h"
#include "pipeline/AudioRenderPipeline.h"
#include "support/TimelineTestSupport.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

namespace {

TimelineSnapshotSpec makeProjectSnapshot(const juce::String& projectId) {
    auto snapshot = makeTestSnapshot();
    snapshot.projectId = projectId;
    return snapshot;
}

}  // namespace

TEST(AudioStatusBuilderTest, ReportsInitialMetersForTheActiveProject) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    juce::String error;

    ASSERT_TRUE(loadTestSnapshot(timeline, makeProjectSnapshot("project:meters"), formats, 48'000.0,
                                 32, error))
        << error.toStdString();

    const auto meters = AudioStatusBuilder::currentMeters(callback, timeline);

    EXPECT_EQ(meters.projectId, std::optional<juce::String>("project:meters"));
    EXPECT_EQ(meters.muteReasons, 0u);
    EXPECT_EQ(meters.invalidSamples, 0u);
}

TEST(AudioStatusBuilderTest, KeepsProjectMeterEpochAndCumulativeDiagnosticsConsistent) {
    juce::AudioDeviceManager manager;
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    juce::String error;

    ASSERT_TRUE(loadTestSnapshot(timeline, makeProjectSnapshot("project:old"), formats, 48'000.0,
                                 32, error))
        << error.toStdString();
    const auto oldProjectEpoch = timeline.activeProjectMeterIdentity().meterEpoch;
    callback.metrics().beginProjectBlock(oldProjectEpoch);
    callback.metrics().recordBlock(oldProjectEpoch, 0.9f, 0.95f, 0.98f, 0.97f, 0.96f, 6.0f, 3, 4);

    ASSERT_TRUE(loadTestSnapshot(timeline, makeProjectSnapshot("project:new"), formats, 48'000.0,
                                 32, error))
        << error.toStdString();

    const auto newProjectEpoch = timeline.activeProjectMeterIdentity().meterEpoch;
    callback.metrics().recordBlock(oldProjectEpoch, 0.9f, 0.95f, 0.98f, 0.97f, 0.96f, 6.0f, 3, 4);

    const auto boundaryMeters = AudioStatusBuilder::currentMeters(callback, timeline);
    EXPECT_EQ(boundaryMeters.projectId, std::optional<juce::String>("project:new"));
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.inputPeak), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.outputPeak), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.preLimiterPeak), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.limiterGainReductionDb), 0.0f);
    EXPECT_EQ(boundaryMeters.invalidSamples, 8u);
    EXPECT_EQ(boundaryMeters.hardClipSamples, 6u);

    callback.metrics().beginProjectBlock(newProjectEpoch);
    callback.metrics().recordBlock(newProjectEpoch, 0.1f, 0.2f, 0.3f, 0.25f, 0.35f, 2.5f, 0, 0);

    MidiInputService midi(callback.preview(), callback.audition(), timeline);
    const auto status =
        AudioStatusBuilder::currentStatus(manager, callback, midi.monitor(), timeline);
    EXPECT_FLOAT_EQ(static_cast<float>(status.inputPeak), 0.1f);
    EXPECT_FLOAT_EQ(static_cast<float>(status.outputPeak), 0.3f);
    EXPECT_FLOAT_EQ(static_cast<float>(status.diagnostics.preLimiterPeak), 0.2f);
    EXPECT_FLOAT_EQ(static_cast<float>(status.diagnostics.limiterGainReductionDb), 2.5f);

    const auto meters = AudioStatusBuilder::currentMeters(callback, timeline);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.inputPeak), 0.1f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.outputPeak), 0.3f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.outputPeakLeft), 0.25f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.outputPeakRight), 0.35f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.preLimiterPeak), 0.2f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.limiterGainReductionDb), 2.5f);
}

}  // namespace riffra
