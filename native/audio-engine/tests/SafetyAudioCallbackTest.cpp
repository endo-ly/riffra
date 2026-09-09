#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <limits>
#include <memory>

#include "AudioRuntimeStatus.h"
#include "SafetyAudioCallback.h"

namespace riffra {
namespace {

constexpr int kBlockSize = 32;

juce::var makeMonitoringSnapshot(const int channelIndex = 0, const bool armed = false) {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* audioInput = new juce::DynamicObject();
    audioInput->setProperty("channelIndex", channelIndex);
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    auto* track = new juce::DynamicObject();
    track->setProperty("id", "track:monitoring");
    track->setProperty("kind", "audio");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", armed);
    track->setProperty("monitoring", "on");
    track->setProperty("audioInput", juce::var(audioInput));
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", juce::Array<juce::var>{});
    track->setProperty("midiClips", juce::Array<juce::var>{});
    track->setProperty("automation", juce::Array<juce::var>{});

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

}  // namespace

TEST(SafetyAudioCallbackTest, HoldsInputTransientUntilStatusCollection) {
    SafetyAudioCallback callback;
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> silence{};
    std::array<float, kBlockSize> output{};
    input.fill(0.5f);
    const std::array<const float*, 1> signalInput{input.data()};
    const std::array<const float*, 1> silentInput{silence.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.audioDeviceIOCallbackWithContext(signalInput.data(), 1, outputs.data(), 1, kBlockSize,
                                              context);
    callback.audioDeviceIOCallbackWithContext(silentInput.data(), 1, outputs.data(), 1, kBlockSize,
                                              context);

    EXPECT_GE(callback.getInputPeak(), 0.5f);
}

TEST(SafetyAudioCallbackTest, SilencesOutputWhenEmergencyMuted) {
    SafetyAudioCallback callback;
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.5f);
    output.fill(1.0f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.audioDeviceIOCallbackWithContext(inputs.data(), 1, outputs.data(), 1, kBlockSize,
                                              context);

    for (const auto sample : output) EXPECT_FLOAT_EQ(sample, 0.0f);
}

TEST(SafetyAudioCallbackTest, ReportsInvalidAudioSamples) {
    SafetyAudioCallback callback;
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(std::numeric_limits<float>::quiet_NaN());
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.audioDeviceIOCallbackWithContext(inputs.data(), 1, outputs.data(), 1, kBlockSize,
                                              context);

    EXPECT_GT(callback.getInvalidSampleCount(), 0u);
    EXPECT_TRUE(std::isfinite(output.front()));
}

TEST(SafetyAudioCallbackTest, DoesNotMuteForAHotInputWhenMonitoringIsOff) {
    SafetyAudioCallback callback;
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    for (int block = 0; block < 400; ++block)
        callback.audioDeviceIOCallbackWithContext(inputs.data(), 1, outputs.data(), 1, kBlockSize,
                                                  context);

    EXPECT_FALSE(callback.isMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(SafetyAudioCallbackTest, ReleasingFeedbackProtectionClearsItsMuteReason) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(
        timeline.loadSnapshot(makeMonitoringSnapshot(), formats, 48'000.0, kBlockSize, error));
    SafetyAudioCallback callback;
    callback.setTimelineEngine(&timeline);
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    for (int block = 0; block < 400; ++block)
        callback.audioDeviceIOCallbackWithContext(inputs.data(), 1, outputs.data(), 1, kBlockSize,
                                                  context);

    ASSERT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
    ASSERT_TRUE(callback.isFeedbackSuspected());

    callback.setUserEmergencyMute(false);
    EXPECT_TRUE(callback.isMuted());
    callback.setFeedbackProtection(false);

    EXPECT_FALSE(callback.isMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(SafetyAudioCallbackTest, DetectsFeedbackOnEveryMonitoredInputChannel) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(
        timeline.loadSnapshot(makeMonitoringSnapshot(1), formats, 48'000.0, kBlockSize, error));
    SafetyAudioCallback callback;
    callback.setTimelineEngine(&timeline);
    callback.setInputChannel(0);
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> selectedInput{};
    std::array<float, kBlockSize> monitoredInput{};
    std::array<float, kBlockSize> output{};
    monitoredInput.fill(0.99f);
    const std::array<const float*, 2> inputs{selectedInput.data(), monitoredInput.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    // Act
    for (int block = 0; block < 400; ++block)
        callback.audioDeviceIOCallbackWithContext(inputs.data(), 2, outputs.data(), 1, kBlockSize,
                                                  context);

    // Assert
    EXPECT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
    EXPECT_TRUE(callback.isFeedbackSuspected());
}

TEST(SafetyAudioCallbackTest, DetachesRecordingBeforeFinalizationCompletes) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(makeMonitoringSnapshot(0, true), formats, 48'000.0,
                                      kBlockSize, error));
    SafetyAudioCallback callback;
    callback.setTimelineEngine(&timeline);
    std::shared_ptr<ArrangeRecordingSession> detached;
    callback.setRecordingFinalizationDispatcher(
        [&detached](std::unique_ptr<ArrangeRecordingSession> session) {
            detached = std::shared_ptr<ArrangeRecordingSession>(std::move(session));
        });
    const auto directory = juce::File::getSpecialLocation(juce::File::tempDirectory)
                               .getChildFile("riffra-safety-recording-detach-test")
                               .getChildFile(juce::Uuid().toString());

    // Act
    ASSERT_TRUE(callback.startArrangeRecording(directory, timeline, error));
    ASSERT_TRUE(timeline.startRecording(0, error));
    ASSERT_TRUE(callback.stopArrangeRecording(timeline, error));

    // Assert
    ASSERT_NE(detached, nullptr);
    const auto processingStatus = callback.recordingStatus();
    EXPECT_FALSE(static_cast<bool>(processingStatus.getProperty("active", false)));
    EXPECT_TRUE(static_cast<bool>(processingStatus.getProperty("processing", false)));

    callback.completeArrangeRecordingProcessing(detached->status(), {});
    EXPECT_FALSE(static_cast<bool>(callback.recordingStatus().getProperty("processing", true)));
    detached.reset();
    directory.deleteRecursively();
}

TEST(SafetyAudioCallbackTest, DeviceFaultRemainsAfterUserMuteRelease) {
    SafetyAudioCallback callback;
    callback.setUserEmergencyMute(false);
    ASSERT_FALSE(callback.isMuted());

    callback.setDeviceFaulted(true);
    callback.setUserEmergencyMute(true);

    callback.setUserEmergencyMute(false);

    EXPECT_TRUE(callback.isMuted());

    callback.setDeviceFaulted(false);
    callback.setUserEmergencyMute(false);

    EXPECT_FALSE(callback.isMuted());
}

TEST(SafetyAudioCallbackTest, RequiresFaultWhenActiveDeviceDisappears) {
    EXPECT_TRUE(deviceLossRequiresFault(false, false));
    EXPECT_FALSE(deviceLossRequiresFault(true, false));
}

TEST(SafetyAudioCallbackTest, DeviceTransitionSuppressesFault) {
    EXPECT_FALSE(deviceLossRequiresFault(false, true));
}

TEST(SafetyAudioCallbackTest, DeviceTransitionSuppressesFaultWithoutInspectingMuteState) {
    SafetyAudioCallback callback;
    callback.setUserEmergencyMute(true);
    callback.setDeviceTransitionActive(true);

    EXPECT_FALSE(deviceLossRequiresFault(false, callback.isDeviceTransitionActive()));

    callback.setDeviceTransitionActive(false);
    EXPECT_TRUE(deviceLossRequiresFault(false, callback.isDeviceTransitionActive()));
}

TEST(SafetyAudioCallbackTest, ReportsDisconnectedDeviceAsFaultedStatus) {
    SafetyAudioCallback callback;
    callback.audioDeviceError("disconnected");

    EXPECT_TRUE(callback.isMuted());
    EXPECT_EQ(callback.takeLastDeviceError(), "disconnected");
}

}  // namespace riffra
