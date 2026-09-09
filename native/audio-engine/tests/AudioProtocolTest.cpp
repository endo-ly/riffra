#include <gtest/gtest.h>

#include "AudioDeviceService.h"
#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "audio/AudioRenderPipeline.h"
#include "TimelineEngine.h"

namespace riffra {

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

TEST(AudioDeviceServiceTest, ReportsSafeInitialMeters) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    const auto meters = AudioDeviceService::currentMeters(callback);

    ASSERT_TRUE(meters.isObject());
    EXPECT_EQ(meters.getProperty("type", {}).toString(), "audioMeters");
    EXPECT_EQ(meters.getProperty("muteReasons", 0).toString().getIntValue(), 0);
    EXPECT_EQ(static_cast<int>(meters.getProperty("invalidSamples", 0)), 0);
}

TEST(AudioDeviceServiceTest, ReportsStableStatusContractWithoutDevice) {
    juce::AudioDeviceManager manager;
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    const auto status = AudioDeviceService::currentStatus(manager, callback);

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
    EXPECT_TRUE(status.hasProperty("recording"));
    EXPECT_TRUE(status.hasProperty("diagnostics"));
    EXPECT_TRUE(status.hasProperty("midiInputs"));
    EXPECT_TRUE(status.hasProperty("midiOutputs"));
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
