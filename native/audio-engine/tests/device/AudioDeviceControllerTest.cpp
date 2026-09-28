#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <thread>

#include "../timeline/TimelineTestSupport.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceCallback.h"
#include "device/AudioDeviceController.h"
#include "recording/ArrangeRecordingSession.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

TimelineSnapshotSpec makeArmedAudioSnapshot() {
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:recording");
    track.armed = true;
    track.audioInput = AudioInputSpec{0};
    snapshot.graph.tracks.push_back(std::move(track));
    return snapshot;
}

TEST(AudioDeviceControllerTest, DeviceLossRequiresFaultOnlyOutsideTransition) {
    EXPECT_TRUE(AudioDeviceController::requiresFaultForState(false, false));
    EXPECT_FALSE(AudioDeviceController::requiresFaultForState(false, true));
    EXPECT_FALSE(AudioDeviceController::requiresFaultForState(true, false));
}

TEST(AudioDeviceControllerTest, DeviceStopHandlerCanFinalizeRecordingAsynchronously) {
    // Arrange
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    TimelineEngine timeline;
    juce::String error;
    ASSERT_TRUE(loadTestSnapshot(timeline, makeArmedAudioSnapshot(), formats, 48'000.0, 32, error));
    AudioRenderPipeline pipeline(timeline);
    std::shared_ptr<ArrangeRecordingSession> detached;
    pipeline.recording().setFinalizationDispatcher(
        [&detached](std::unique_ptr<ArrangeRecordingSession> session) {
            detached = std::shared_ptr<ArrangeRecordingSession>(std::move(session));
        });
    const auto directory = juce::File::getSpecialLocation(juce::File::tempDirectory)
                               .getChildFile("riffra-device-stop-recording-test")
                               .getChildFile(juce::Uuid().toString());
    ASSERT_TRUE(pipeline.recording().start(directory, error));
    ASSERT_TRUE(timeline.startRecording(0, error));
    std::array<float, 32> input{};
    std::array<float, 32> output{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 1> outputs{output.data()};
    const juce::AudioIODeviceCallbackContext context{};
    pipeline.processBlock(inputs.data(), 1, outputs.data(), 1, static_cast<int>(input.size()),
                          context);

    bool handlerCalled = false;
    std::thread deferredStop;
    AudioDeviceCallback callback(pipeline, [&] {
        handlerCalled = true;
        deferredStop = std::thread([&pipeline] {
            juce::String ignored;
            (void)pipeline.recording().stop(ignored);
        });
    });

    // Act
    callback.audioDeviceStopped();
    ASSERT_TRUE(handlerCalled);
    ASSERT_TRUE(deferredStop.joinable());
    deferredStop.join();

    // Assert
    ASSERT_NE(detached, nullptr);
    const auto status = pipeline.recording().status();
    EXPECT_FALSE(status.active);
    EXPECT_TRUE(status.processing);

    juce::String finalizationError;
    ASSERT_TRUE(timeline.processFinalizedRecording(detached.get(), finalizationError))
        << finalizationError;
    ASSERT_TRUE(detached->finish(true, finalizationError)) << finalizationError;
    EXPECT_TRUE(directory.getChildFile("tracks/0000/raw.wav").existsAsFile());
    EXPECT_TRUE(directory.getChildFile("tracks/0000/processed.wav").existsAsFile());
    pipeline.recording().completeProcessing(detached->summary(), finalizationError);
    detached.reset();
    directory.deleteRecursively();
}

}  // namespace
}  // namespace riffra
