#include "SafetyAudioCallback.h"
#include "AudioRuntimeStatus.h"

#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <limits>

namespace riffra {
namespace {

constexpr int kBlockSize = 32;

juce::var makeMonitoringSnapshot() {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* audioInput = new juce::DynamicObject();
    audioInput->setProperty("channelIndex", 0);
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var> {});
    auto* track = new juce::DynamicObject();
    track->setProperty("id", "track:monitoring");
    track->setProperty("kind", "audio");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", false);
    track->setProperty("monitoring", "on");
    track->setProperty("audioInput", juce::var(audioInput));
    track->setProperty("rack", juce::var(rack));
    track->setProperty("audioClips", juce::Array<juce::var> {});
    track->setProperty("midiClips", juce::Array<juce::var> {});
    track->setProperty("automation", juce::Array<juce::var> {});

    juce::Array<juce::var> tracks;
    tracks.add(juce::var(track));
    auto* snapshot = new juce::DynamicObject();
    snapshot->setProperty("revision", 1);
    snapshot->setProperty("timebase", juce::var(timebase));
    snapshot->setProperty("tracks", tracks);
    return juce::var(snapshot);
}

} // namespace

TEST(SafetyAudioCallbackTest, HoldsInputTransientUntilStatusCollection)
{
    SafetyAudioCallback callback;
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> silence {};
    std::array<float, kBlockSize> output {};
    input.fill(0.5f);
    const std::array<const float*, 1> signalInput { input.data() };
    const std::array<const float*, 1> silentInput { silence.data() };
    const std::array<float*, 1> outputs { output.data() };
    const juce::AudioIODeviceCallbackContext context {};

    callback.audioDeviceIOCallbackWithContext(
        signalInput.data(), 1, outputs.data(), 1, kBlockSize, context);
    callback.audioDeviceIOCallbackWithContext(
        silentInput.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_GE(callback.getInputPeak(), 0.5f);
}

TEST(SafetyAudioCallbackTest, SilencesOutputWhenEmergencyMuted)
{
    SafetyAudioCallback callback;
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> output {};
    input.fill(0.5f);
    output.fill(1.0f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 1> outputs { output.data() };
    const juce::AudioIODeviceCallbackContext context {};

    callback.audioDeviceIOCallbackWithContext(
        inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    for (const auto sample : output)
        EXPECT_FLOAT_EQ(sample, 0.0f);
}

TEST(SafetyAudioCallbackTest, ReportsInvalidAudioSamples)
{
    SafetyAudioCallback callback;
    callback.setEmergencyMuted(false);
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> output {};
    input.fill(std::numeric_limits<float>::quiet_NaN());
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 1> outputs { output.data() };
    const juce::AudioIODeviceCallbackContext context {};

    callback.audioDeviceIOCallbackWithContext(
        inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_GT(callback.getInvalidSampleCount(), 0u);
    EXPECT_TRUE(std::isfinite(output.front()));
}

TEST(SafetyAudioCallbackTest, DoesNotMuteForAHotInputWhenMonitoringIsOff)
{
    SafetyAudioCallback callback;
    callback.setEmergencyMuted(false);
    callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::passive);
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> output {};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 1> outputs { output.data() };
    const juce::AudioIODeviceCallbackContext context {};

    for (int block = 0; block < 400; ++block)
        callback.audioDeviceIOCallbackWithContext(
            inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_FALSE(callback.isEmergencyMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(SafetyAudioCallbackTest, ReleasingEmergencyMuteClearsFeedbackCause)
{
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(
        makeMonitoringSnapshot(), formats, 48'000.0, kBlockSize, error));
    SafetyAudioCallback callback;
    callback.setTimelineEngine(&timeline);
    callback.setEmergencyMuted(false);
    callback.setProcessingMode(SafetyAudioCallback::ProcessingMode::arrange);
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> output {};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 1> outputs { output.data() };
    const juce::AudioIODeviceCallbackContext context {};

    for (int block = 0; block < 400; ++block)
        callback.audioDeviceIOCallbackWithContext(
            inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    ASSERT_TRUE(callback.isEmergencyMuted());
    ASSERT_TRUE(callback.isFeedbackSuspected());

    callback.setEmergencyMuted(false);

    EXPECT_FALSE(callback.isEmergencyMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(SafetyAudioCallbackTest, DeviceFaultKeepsEmergencyMuteEngaged)
{
    SafetyAudioCallback callback;
    callback.setEmergencyMuted(false);
    ASSERT_FALSE(callback.isEmergencyMuted());

    callback.setDeviceFaulted(true);
    callback.setEmergencyMuted(true);

    callback.setEmergencyMuted(false);

    EXPECT_TRUE(callback.isEmergencyMuted());

    callback.setDeviceFaulted(false);
    callback.setEmergencyMuted(false);

    EXPECT_FALSE(callback.isEmergencyMuted());
}

TEST(SafetyAudioCallbackTest, RequiresFaultWhenActiveDeviceDisappears)
{
    EXPECT_TRUE(deviceLossRequiresFault(false, true));
    EXPECT_FALSE(deviceLossRequiresFault(true, true));
}

TEST(SafetyAudioCallbackTest, DoesNotFaultWhileMutedAndIdle)
{
    EXPECT_FALSE(deviceLossRequiresFault(false, false));
}

TEST(SafetyAudioCallbackTest, ReportsDisconnectedDeviceAsFaultedStatus)
{
    SafetyAudioCallback callback;
    callback.audioDeviceError("disconnected");

    EXPECT_TRUE(callback.isEmergencyMuted());
    EXPECT_EQ(callback.takeLastDeviceError(), "disconnected");
}

} // namespace riffra
