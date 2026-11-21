#include <gtest/gtest.h>

#include <algorithm>
#include <array>

#include "device/MidiInputService.h"
#include "pipeline/AudioRenderPipeline.h"
#include "support/TimelineTestSupport.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

TEST(MidiInputServiceTest, ProcessesMonitoredNotesWithoutListeningForDevices) {
    TimelineEngine timeline;
    AudioRenderPipeline callback(timeline);
    MidiInputService service(callback.preview(), callback.audition(), timeline);
    auto& monitor = service.monitor();

    monitor.setActive(true);
    monitor.receive(0, juce::MidiMessage::noteOn(1, 60, 0.8f));

    std::array<float, 256> output{};
    const std::array<float*, 1> outputs{output.data()};
    ASSERT_TRUE(callback.preview().tryMix(outputs.data(), 1, 256, 48'000.0));
    EXPECT_TRUE(std::any_of(output.begin(), output.end(),
                            [](const float sample) { return sample != 0.0f; }));

    monitor.receive(0, juce::MidiMessage::noteOff(1, 60));
    for (int block = 0; block < 8; ++block) {
        output.fill(0.0f);
        ASSERT_TRUE(callback.preview().tryMix(outputs.data(), 1, 256, 48'000.0));
    }
    EXPECT_TRUE(std::all_of(output.begin(), output.end(),
                            [](const float sample) { return sample == 0.0f; }));
    EXPECT_FALSE(callback.preview().isPreviewing());

    EXPECT_TRUE(monitor.isActive());
    EXPECT_EQ(monitor.getMessageCount(), 2u);
    EXPECT_EQ(monitor.getLastNote(), 60);
    EXPECT_FALSE(service.isListening());
    EXPECT_FALSE(service.deviceSetChanged());
}

}  // namespace riffra
