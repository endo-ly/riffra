#include <gtest/gtest.h>

#include "app/AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceService.h"
#include "midi/MidiInputService.h"
#include "protocol/AudioProtocol.h"
#include "protocol/OutputQueue.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

namespace {

juce::var makeProjectSnapshot(const juce::String& projectId) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("projectId", projectId);
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", juce::Array<juce::var>{});
    return juce::var(snapshot);
}

}  // namespace

TEST(AudioProtocolTest, ParsesThreeByteNoteMessage) {
    juce::Array<juce::var> bytes;
    bytes.add(0x90);
    bytes.add(60);
    bytes.add(100);
    juce::MidiMessage message;
    juce::String error;

    const auto parsed = parseMidiBytes(juce::var(bytes), message, error);

    EXPECT_TRUE(parsed);
    EXPECT_TRUE(message.isNoteOn());
    EXPECT_EQ(message.getNoteNumber(), 60);
    EXPECT_EQ(message.getVelocity(), 100);
    EXPECT_TRUE(error.isEmpty());
}

TEST(AudioProtocolTest, RejectsMidiDataBytesAboveSevenBits) {
    juce::Array<juce::var> bytes;
    bytes.add(0x90);
    bytes.add(128);
    juce::MidiMessage message;
    juce::String error;

    const auto parsed = parseMidiBytes(juce::var(bytes), message, error);

    EXPECT_FALSE(parsed);
    EXPECT_EQ(error, "MIDI data bytes must be below 128.");
}

TEST(AudioProtocolTest, CreatesSafeErrorPayload) {
    const auto error = makeError("protocol", "invalid request");

    ASSERT_TRUE(error.isObject());
    EXPECT_EQ(error.getProperty("type", {}).toString(), "error");
    EXPECT_EQ(error.getProperty("kind", {}).toString(), "protocol");
    EXPECT_EQ(error.getProperty("message", {}).toString(), "invalid request");
    EXPECT_EQ(error.getProperty("operation", {}).toString(), "protocol");
    EXPECT_TRUE(error.getProperty("details", {}).isObject());
}

TEST(AudioProtocolTest, ControlOutputDiscardsQueuedTelemetry) {
    OutputQueue queue;

    ASSERT_TRUE(queue.enqueueTelemetry("old telemetry"));
    EXPECT_EQ(queue.enqueueControl("new control"), 1u);
    ASSERT_TRUE(queue.hasControl());
    EXPECT_EQ(queue.takeControl(), "new control");
    EXPECT_FALSE(queue.hasTelemetry());
}

TEST(AudioDeviceServiceTest, ReportsSafeInitialMeterAndStatusContracts) {
    juce::AudioDeviceManager manager;
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    juce::String error;

    ASSERT_TRUE(
        timeline.loadSnapshot(makeProjectSnapshot("project:meters"), formats, 48'000.0, 32, error))
        << error.toStdString();

    const auto meters = AudioStatusBuilder::currentMeters(callback, &timeline);
    const auto status = AudioStatusBuilder::currentStatus(manager, callback);

    ASSERT_TRUE(meters.isObject());
    EXPECT_EQ(meters.getProperty("type", {}).toString(), "audioMeters");
    EXPECT_EQ(meters.getProperty("muteReasons", 0).toString().getIntValue(), 0);
    EXPECT_EQ(static_cast<int>(meters.getProperty("invalidSamples", 0)), 0);
    EXPECT_TRUE(meters.getProperty("projectId", {}).toString() == juce::String("project:meters"));
    EXPECT_TRUE(meters.hasProperty("outputPeakLeft"));
    EXPECT_TRUE(meters.hasProperty("outputPeakRight"));
    EXPECT_TRUE(meters.hasProperty("trackMeters"));
    ASSERT_TRUE(status.isObject());
    EXPECT_EQ(status.getProperty("type", {}).toString(), "audioStatus");
    EXPECT_TRUE(status.hasProperty("state"));
    EXPECT_TRUE(status.hasProperty("muteReasons"));
    EXPECT_TRUE(status.hasProperty("masterGainDb"));
    EXPECT_TRUE(status.hasProperty("inputPeak"));
    EXPECT_TRUE(status.hasProperty("outputPeak"));
    EXPECT_TRUE(status.hasProperty("invalidSamples"));
    EXPECT_TRUE(status.hasProperty("feedbackSuspected"));
    EXPECT_TRUE(status.hasProperty("previewing"));
    EXPECT_TRUE(status.hasProperty("builtInPreviewing"));
    EXPECT_TRUE(status.hasProperty("recording"));
    EXPECT_TRUE(status.hasProperty("diagnostics"));
    EXPECT_TRUE(status.hasProperty("midiInputs"));
    EXPECT_TRUE(status.hasProperty("midiOutputs"));
}

TEST(AudioDeviceServiceTest, KeepsProjectMeterEpochAndCumulativeDiagnosticsConsistent) {
    juce::AudioDeviceManager manager;
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    juce::String error;

    ASSERT_TRUE(
        timeline.loadSnapshot(makeProjectSnapshot("project:old"), formats, 48'000.0, 32, error))
        << error.toStdString();
    const auto oldProjectEpoch = timeline.activeProjectMeterIdentity().meterEpoch;
    callback.metrics().beginProjectBlock(oldProjectEpoch);
    callback.metrics().recordBlock(oldProjectEpoch, 0.9f, 0.95f, 0.98f, 0.97f, 0.96f, 6.0f, 3, 4);

    ASSERT_TRUE(
        timeline.loadSnapshot(makeProjectSnapshot("project:new"), formats, 48'000.0, 32, error))
        << error.toStdString();

    const auto newProjectEpoch = timeline.activeProjectMeterIdentity().meterEpoch;
    callback.metrics().recordBlock(oldProjectEpoch, 0.9f, 0.95f, 0.98f, 0.97f, 0.96f, 6.0f, 3, 4);

    const auto boundaryMeters = AudioStatusBuilder::currentMeters(callback, &timeline);
    EXPECT_TRUE(boundaryMeters.getProperty("projectId", {}).toString() ==
                juce::String("project:new"));
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.getProperty("inputPeak", 0.0)), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.getProperty("outputPeak", 0.0)), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.getProperty("preLimiterPeak", 0.0)), 0.0f);
    EXPECT_FLOAT_EQ(static_cast<float>(boundaryMeters.getProperty("limiterGainReductionDb", 0.0)),
                    0.0f);
    EXPECT_EQ(static_cast<int>(boundaryMeters.getProperty("invalidSamples", 0)), 8);
    EXPECT_EQ(static_cast<int>(boundaryMeters.getProperty("hardClipSamples", 0)), 6);

    callback.metrics().beginProjectBlock(newProjectEpoch);
    callback.metrics().recordBlock(newProjectEpoch, 0.1f, 0.2f, 0.3f, 0.25f, 0.35f, 2.5f, 0, 0);

    const auto status =
        AudioStatusBuilder::currentStatus(manager, callback, nullptr, {}, &timeline);
    const auto* diagnostics = status.getProperty("diagnostics", {}).getDynamicObject();
    ASSERT_NE(diagnostics, nullptr);
    EXPECT_FLOAT_EQ(static_cast<float>(status.getProperty("inputPeak", 0.0)), 0.1f);
    EXPECT_FLOAT_EQ(static_cast<float>(status.getProperty("outputPeak", 0.0)), 0.3f);
    EXPECT_FLOAT_EQ(static_cast<float>(diagnostics->getProperty("preLimiterPeak")), 0.2f);
    EXPECT_FLOAT_EQ(static_cast<float>(diagnostics->getProperty("limiterGainReductionDb")), 2.5f);

    const auto meters = AudioStatusBuilder::currentMeters(callback, &timeline);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("inputPeak", 0.0)), 0.1f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("outputPeak", 0.0)), 0.3f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("outputPeakLeft", 0.0)), 0.25f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("outputPeakRight", 0.0)), 0.35f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("preLimiterPeak", 0.0)), 0.2f);
    EXPECT_FLOAT_EQ(static_cast<float>(meters.getProperty("limiterGainReductionDb", 0.0)), 2.5f);
}

TEST(AudioDeviceServiceTest, ReportsProbeFieldsRequiredByTheHost) {
    const auto probe = AudioDeviceService::discover();

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
    monitor.handleIncomingMidiMessage(nullptr, juce::MidiMessage::noteOn(1, 60, 0.8f));

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
