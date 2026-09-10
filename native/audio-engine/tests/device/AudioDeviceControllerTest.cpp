#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <thread>

#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceCallback.h"
#include "device/AudioDeviceController.h"
#include "recording/ArrangeRecordingSession.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

juce::var makeArmedAudioSnapshot() {
    auto* timebase = new juce::DynamicObject();
    timebase->setProperty("ppq", 960);
    timebase->setProperty("bpm", 120.0);
    timebase->setProperty("timeSignatureNumerator", 4);
    timebase->setProperty("timeSignatureDenominator", 4);

    auto* audioInput = new juce::DynamicObject();
    audioInput->setProperty("channelIndex", 0);
    auto* rack = new juce::DynamicObject();
    rack->setProperty("devices", juce::Array<juce::var>{});
    auto* track = new juce::DynamicObject();
    track->setProperty("id", "track:recording");
    track->setProperty("kind", "audio");
    track->setProperty("gainDb", 0.0);
    track->setProperty("pan", 0.0);
    track->setProperty("muted", false);
    track->setProperty("solo", false);
    track->setProperty("armed", true);
    track->setProperty("monitoring", "off");
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
    ASSERT_TRUE(timeline.loadSnapshot(makeArmedAudioSnapshot(), formats, 48'000.0, 32, error));
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
    EXPECT_FALSE(static_cast<bool>(status.getProperty("active", false)));
    EXPECT_TRUE(static_cast<bool>(status.getProperty("processing", false)));

    juce::String finalizationError;
    ASSERT_TRUE(timeline.processFinalizedRecording(detached.get(), finalizationError))
        << finalizationError;
    ASSERT_TRUE(detached->finish(true, finalizationError)) << finalizationError;
    EXPECT_TRUE(directory.getChildFile("tracks/0000/raw.wav").existsAsFile());
    EXPECT_TRUE(directory.getChildFile("tracks/0000/processed.wav").existsAsFile());
    pipeline.recording().completeProcessing(detached->status(), finalizationError);
    detached.reset();
    directory.deleteRecursively();
}

}  // namespace
}  // namespace riffra
