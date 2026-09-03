#include <gtest/gtest.h>

#include "AudioDeviceService.h"
#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "SafetyAudioCallback.h"
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
    EXPECT_EQ(error.getProperty("scope", {}).toString(), "protocol");
    EXPECT_EQ(error.getProperty("message", {}).toString(), "invalid request");
    EXPECT_TRUE(static_cast<bool>(error.getProperty("dataSafe", false)));
}

TEST(AudioDeviceServiceTest, ReportsSafeInitialMeters) {
    SafetyAudioCallback callback;

    const auto meters = AudioDeviceService::currentMeters(callback);

    ASSERT_TRUE(meters.isObject());
    EXPECT_EQ(meters.getProperty("type", {}).toString(), "audioMeters");
    EXPECT_TRUE(static_cast<bool>(meters.getProperty("emergencyMuted", false)));
    EXPECT_EQ(static_cast<int>(meters.getProperty("invalidSamples", 0)), 0);
}

TEST(MidiInputServiceTest, TracksMonitorStateAndNoteMessages) {
    SafetyAudioCallback callback;
    TimelineEngine timeline;
    MidiInputService service(callback, timeline);
    auto& monitor = service.monitor();

    monitor.setActive(true);
    monitor.handleIncomingMidiMessage(nullptr, juce::MidiMessage::noteOn(1, 60, 0.8f));

    EXPECT_TRUE(monitor.isActive());
    EXPECT_EQ(monitor.getMessageCount(), 1u);
    EXPECT_EQ(monitor.getLastNote(), 60);
}

TEST(MidiInputServiceTest, StartsWithoutListeningForDevices) {
    SafetyAudioCallback callback;
    TimelineEngine timeline;
    MidiInputService service(callback, timeline);

    EXPECT_FALSE(service.isListening());
    EXPECT_FALSE(service.deviceSetChanged());
}

}  // namespace riffra
