#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <limits>
#include <memory>

#include "ArrangeRecordingSession.h"
#include "AudioRuntimeStatus.h"
#include "audio/AudioRenderPipeline.h"
#include "TimelineEngine.h"

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

TEST(AudioRenderPipelineTest, HoldsInputTransientUntilStatusCollection) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> silence{};
    std::array<float, kBlockSize> output{};
    input.fill(0.5f);
    const std::array<const float*, 1> signalInput{input.data()};
    const std::array<const float*, 1> silentInput{silence.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.processBlock(signalInput.data(), 1, outputs.data(), 1, kBlockSize, context);
    callback.processBlock(silentInput.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_GE(callback.getInputPeak(), 0.5f);
}

TEST(AudioRenderPipelineTest, SilencesOutputWhenEmergencyMuted) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.5f);
    output.fill(1.0f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.processBlock(inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    for (const auto sample : output) EXPECT_FLOAT_EQ(sample, 0.0f);
}

TEST(AudioRenderPipelineTest, ReportsInvalidAudioSamples) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(std::numeric_limits<float>::quiet_NaN());
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    callback.processBlock(inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_GT(callback.getInvalidSampleCount(), 0u);
    EXPECT_TRUE(std::isfinite(output.front()));
}

TEST(AudioRenderPipelineTest, DoesNotMuteForAHotInputWhenMonitoringIsOff) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    for (int block = 0; block < 400; ++block)
        callback.processBlock(inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    EXPECT_FALSE(callback.isMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(AudioRenderPipelineTest, ReleasingFeedbackProtectionClearsItsMuteReason) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(
        timeline.loadSnapshot(makeMonitoringSnapshot(), formats, 48'000.0, kBlockSize, error));
    AudioRenderPipeline callback(timeline);
    callback.setUserEmergencyMute(false);
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> output{};
    input.fill(0.99f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};

    for (int block = 0; block < 400; ++block)
        callback.processBlock(inputs.data(), 1, outputs.data(), 1, kBlockSize, context);

    ASSERT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
    ASSERT_TRUE(callback.isFeedbackSuspected());

    callback.setUserEmergencyMute(false);
    EXPECT_TRUE(callback.isMuted());
    callback.setFeedbackProtection(false);

    EXPECT_FALSE(callback.isMuted());
    EXPECT_FALSE(callback.isFeedbackSuspected());
}

TEST(AudioRenderPipelineTest, DetectsFeedbackOnEveryMonitoredInputChannel) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(
        timeline.loadSnapshot(makeMonitoringSnapshot(1), formats, 48'000.0, kBlockSize, error));
    AudioRenderPipeline callback(timeline);
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
        callback.processBlock(inputs.data(), 2, outputs.data(), 1, kBlockSize, context);

    // Assert
    EXPECT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
    EXPECT_TRUE(callback.isFeedbackSuspected());
}

TEST(AudioRenderPipelineTest, DetachesRecordingBeforeFinalizationCompletes) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(makeMonitoringSnapshot(0, true), formats, 48'000.0,
                                      kBlockSize, error));
    AudioRenderPipeline callback(timeline);
    std::shared_ptr<ArrangeRecordingSession> detached;
    callback.setRecordingFinalizationDispatcher(
        [&detached](std::unique_ptr<ArrangeRecordingSession> session) {
            detached = std::shared_ptr<ArrangeRecordingSession>(std::move(session));
        });
    const auto directory = juce::File::getSpecialLocation(juce::File::tempDirectory)
                               .getChildFile("riffra-safety-recording-detach-test")
                               .getChildFile(juce::Uuid().toString());

    // Act
    ASSERT_TRUE(callback.recording().start(directory, error));
    ASSERT_TRUE(timeline.startRecording(0, error));
    ASSERT_TRUE(callback.recording().stop(error));

    // Assert
    ASSERT_NE(detached, nullptr);
    const auto processingStatus = callback.recording().status();
    EXPECT_FALSE(static_cast<bool>(processingStatus.getProperty("active", false)));
    EXPECT_TRUE(static_cast<bool>(processingStatus.getProperty("processing", false)));

    callback.recording().completeProcessing(detached->status(), {});
    EXPECT_FALSE(static_cast<bool>(callback.recording().status().getProperty("processing", true)));
    detached.reset();
    directory.deleteRecursively();
}

TEST(AudioRenderPipelineTest, DeviceFaultRemainsAfterUserMuteRelease) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
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

TEST(AudioRenderPipelineTest, MuteReasonsAreIndependent) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    callback.setUserEmergencyMute(true);
    callback.setEngineTransitionMute(true);
    callback.setDeviceFaulted(true);
    callback.setFeedbackProtection(true);

    callback.setUserEmergencyMute(false);

    EXPECT_FALSE(callback.hasMuteReason(MuteReason::UserEmergency));
    EXPECT_TRUE(callback.hasMuteReason(MuteReason::EngineTransition));
    EXPECT_TRUE(callback.hasMuteReason(MuteReason::DeviceFault));
    EXPECT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
}

TEST(AudioRenderPipelineTest, ClearingUserMuteDoesNotClearEngineTransition) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    callback.setEngineTransitionMute(true);
    callback.setUserEmergencyMute(false);

    EXPECT_TRUE(callback.hasMuteReason(MuteReason::EngineTransition));
}

TEST(AudioRenderPipelineTest, ClearingUserMuteDoesNotClearFeedbackProtection) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    callback.setFeedbackProtection(true);
    callback.setUserEmergencyMute(false);

    EXPECT_TRUE(callback.hasMuteReason(MuteReason::FeedbackProtection));
}

TEST(AudioRenderPipelineTest, MasterGainClampsToMinus90AndZero) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);

    callback.setMasterGainDb(-120.0f);
    EXPECT_FLOAT_EQ(callback.getMasterGainDb(), -90.0f);

    callback.setMasterGainDb(12.0f);
    EXPECT_FLOAT_EQ(callback.getMasterGainDb(), 0.0f);
}

TEST(AudioRenderPipelineTest, PreviewUsesExistingVoiceForSameKey) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioBuffer<float> source(1, 16);
    source.clear();
    juce::String error;

    ASSERT_TRUE(callback.startPreview(source, 2, 10, 1.0f, true, error, 7));
    ASSERT_TRUE(callback.startPreview(source, 4, 12, 1.0f, true, error, 7));

    EXPECT_TRUE(callback.isPreviewing());
}

TEST(AudioRenderPipelineTest, PreviewUsesFreeVoicesBeforeStealing) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioBuffer<float> source(1, 16);
    source.clear();
    juce::String error;

    for (int key = 0; key < 8; ++key)
        ASSERT_TRUE(
            callback.startPreview(source, 0, source.getNumSamples(), 1.0f, true, error, key));

    ASSERT_TRUE(callback.startPreview(source, 0, source.getNumSamples(), 1.0f, true, error, 8));
    EXPECT_TRUE(callback.isPreviewing());
}

TEST(AudioRenderPipelineTest, PreviewSwitchPreservesRelativeCursor) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    juce::AudioBuffer<float> source(1, 16);
    juce::AudioBuffer<float> replacement(1, 32);
    source.clear();
    replacement.clear();
    juce::String error;
    ASSERT_TRUE(callback.startPreview(source, 2, 10, 1.0f, true, error, 7));
    std::array<float*, 1> output{replacement.getWritePointer(0)};
    const juce::AudioIODeviceCallbackContext context{};

    callback.processBlock(nullptr, 0, output.data(), 1, 3, context);

    ASSERT_TRUE(callback.switchPreviewBuffer(7, replacement, error));
    EXPECT_TRUE(callback.isPreviewing());
}

TEST(AudioRenderPipelineTest, SecondRecordingIsRejectedWhileProcessing) {
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(makeMonitoringSnapshot(0, true), formats, 48'000.0,
                                      kBlockSize, error));
    AudioRenderPipeline callback(timeline);
    callback.recording().setFinalizationDispatcher(
        [](std::unique_ptr<ArrangeRecordingSession>) {});
    const auto firstDirectory = juce::File::getSpecialLocation(juce::File::tempDirectory)
                                    .getChildFile("riffra-recording-busy-test")
                                    .getChildFile(juce::Uuid().toString());
    const auto secondDirectory = firstDirectory.getSiblingFile(juce::Uuid().toString());

    ASSERT_TRUE(callback.recording().start(firstDirectory, error));
    ASSERT_TRUE(timeline.startRecording(0, error));
    ASSERT_TRUE(callback.recording().stop(error));
    EXPECT_FALSE(callback.recording().start(secondDirectory, error));

    callback.recording().completeProcessing({}, {});
    firstDirectory.deleteRecursively();
    secondDirectory.deleteRecursively();
}

TEST(AudioRenderPipelineTest, CancelClearsRecordingSink) {
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(timeline.loadSnapshot(makeMonitoringSnapshot(0, true), formats, 48'000.0,
                                      kBlockSize, error));
    AudioRenderPipeline callback(timeline);
    const auto directory = juce::File::getSpecialLocation(juce::File::tempDirectory)
                               .getChildFile("riffra-recording-cancel-test")
                               .getChildFile(juce::Uuid().toString());

    ASSERT_TRUE(callback.recording().start(directory, error));
    ASSERT_TRUE(callback.recording().cancel(error));
    EXPECT_TRUE(callback.recording().status().getProperty("cancelled", false));

    directory.deleteRecursively();
}

TEST(AudioRenderPipelineTest, FinalizationFailurePreservesStatus) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    auto* status = new juce::DynamicObject();
    status->setProperty("directory", "recording");
    status->setProperty("active", true);
    status->setProperty("processing", true);

    callback.recording().completeProcessing(juce::var(status), "finalization failed");
    const auto result = callback.recording().status();

    EXPECT_FALSE(static_cast<bool>(result.getProperty("processing", true)));
    EXPECT_EQ(result.getProperty("error", {}).toString(), "finalization failed");
}

TEST(AudioRenderPipelineTest, RequiresFaultWhenActiveDeviceDisappears) {
    EXPECT_TRUE(deviceLossRequiresFault(false, false));
    EXPECT_FALSE(deviceLossRequiresFault(true, false));
}

TEST(AudioRenderPipelineTest, DeviceTransitionSuppressesFault) {
    EXPECT_FALSE(deviceLossRequiresFault(false, true));
}

TEST(AudioRenderPipelineTest, DeviceTransitionSuppressesFaultWithoutInspectingMuteState) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    callback.setUserEmergencyMute(true);
    callback.setDeviceTransitionActive(true);

    EXPECT_FALSE(deviceLossRequiresFault(false, callback.isDeviceTransitionActive()));

    callback.setDeviceTransitionActive(false);
    EXPECT_TRUE(deviceLossRequiresFault(false, callback.isDeviceTransitionActive()));
}

TEST(AudioRenderPipelineTest, DeviceFaultEngagesDeviceFault) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    callback.setDeviceFaulted(true);

    EXPECT_TRUE(callback.isMuted());
    EXPECT_TRUE(callback.isDeviceFaulted());
}

}  // namespace riffra
