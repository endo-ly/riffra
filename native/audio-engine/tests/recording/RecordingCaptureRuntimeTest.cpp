#include <gtest/gtest.h>

#include "../timeline/TimelineTestSupport.h"
#include "concurrency/RealtimeCommandQueue.h"
#include "timeline/RealtimeCommand.h"

namespace riffra {

TEST(RecordingCaptureRuntimeTest, ReturnsReplacedSinksWithoutLosingAudioWrites) {
    // Arrange
    RecordingCaptureRuntime runtime;
    auto first = std::make_unique<CaptureIsolationSink>();
    auto second = std::make_unique<CaptureIsolationSink>();
    runtime.setSink(first.get());
    RealtimeCommandQueue<RealtimeCommand, 256> commands;
    std::atomic<bool> swapReady{false};
    const juce::String trackId("track:recording");
    const float sample = 0.25f;

    // Act
    std::thread audio([&] {
        for (int index = 0; index < 2000; ++index) {
            if (index == 100) {
                swapReady.store(true, std::memory_order_release);
                bool applied = false;
                while (!applied) {
                    commands.drain([&](const RealtimeCommand& command) {
                        runtime.setSink(command.recordingSink);
                        applied = true;
                    });
                    if (!applied) std::this_thread::yield();
                }
            }
            runtime.writeAudioTrack(trackId, &sample, 1);
        }
    });
    while (!swapReady.load(std::memory_order_acquire)) std::this_thread::yield();
    RealtimeCommand replacement;
    replacement.kind = RealtimeCommand::Kind::setRecordingSink;
    replacement.recordingSink = second.get();
    const auto queued = commands.tryPush(replacement);
    const auto retired = runtime.waitForRetiredSink(first.get());
    const auto firstSamples = first->totalRawSamples;
    if (retired) first.reset();
    audio.join();
    const auto secondSamples = second->totalRawSamples;
    runtime.clearSink();
    const auto secondRetired = runtime.waitForRetiredSink(second.get());

    // Assert
    EXPECT_TRUE(queued);
    EXPECT_TRUE(retired);
    EXPECT_EQ(firstSamples, 100);
    EXPECT_EQ(secondSamples, 1900);
    EXPECT_TRUE(secondRetired);
}

}  // namespace riffra
