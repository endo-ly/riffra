#include <gtest/gtest.h>

#include "../timeline/TimelineTestSupport.h"
#include "app/AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceService.h"
#include "midi/MidiInputService.h"
#include "protocol/OutputQueue.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

namespace {

TimelineSnapshotSpec makeProjectSnapshot(const juce::String& projectId) {
    auto snapshot = makeTestSnapshot();
    snapshot.projectId = projectId;
    return snapshot;
}

}  // namespace

TEST(AudioProtocolTest, ControlOutputDiscardsQueuedTelemetry) {
    OutputQueue queue;

    queue.enqueueTelemetry("old telemetry");
    queue.enqueueControl("new control");
    ASSERT_TRUE(queue.hasControl());
    EXPECT_EQ(queue.takeControl(), "new control");
    EXPECT_FALSE(queue.hasTelemetry());
}

TEST(AudioDeviceServiceTest, ReportsInitialMetersForTheActiveProject) {
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

TEST(AudioDeviceServiceTest, KeepsProjectMeterEpochAndCumulativeDiagnosticsConsistent) {
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

    MidiInputService midi(callback.preview(), timeline);
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

TEST(AudioDeviceServiceTest, ReportsProbeFieldsRequiredByTheHost) {
    const auto probe = encodeAudioDeviceProbe(AudioDeviceService::discover());

    ASSERT_TRUE(probe.isObject());
    EXPECT_EQ(probe.getProperty("type", {}).toString(), "audioDeviceProbe");
    EXPECT_TRUE(static_cast<bool>(probe.getProperty("drivers", {}).isArray()));
    EXPECT_GT(static_cast<juce::int64>(probe.getProperty("refreshedAtMs", 0)), 0);
    EXPECT_EQ(probe.getProperty("message", {}).toString(), "Audio device list refreshed.");
}

TEST(MidiInputServiceTest, TracksMonitorStateAndNoteMessages) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    MidiInputService service(callback.preview(), timeline);
    auto& monitor = service.monitor();

    monitor.setActive(true);
    monitor.receive(0, juce::MidiMessage::noteOn(1, 60, 0.8f));

    EXPECT_TRUE(monitor.isActive());
    EXPECT_EQ(monitor.getMessageCount(), 1u);
    EXPECT_EQ(monitor.getLastNote(), 60);
}

TEST(MidiInputServiceTest, StartsWithoutListeningForDevices) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    MidiInputService service(callback.preview(), timeline);

    EXPECT_FALSE(service.isListening());
    EXPECT_FALSE(service.deviceSetChanged());
}

}  // namespace riffra
