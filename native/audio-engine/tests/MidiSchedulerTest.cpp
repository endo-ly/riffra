#include <gtest/gtest.h>

#include <vector>

#include "MidiScheduler.h"

namespace riffra {
namespace {

MidiClip makeClip(const bool loop) {
    MidiClip clip;
    clip.durationTicks = 4;
    clip.loop = loop;
    clip.notes.push_back({0, 4, 60, 100, 1});
    clip.events.push_back({"controlChange", 1, 1, 7, 100});
    clip.events.push_back({"pitchBend", 2, 1, 0, 64});
    clip.events.push_back({"channelPressure", 3, 1, 80, 0});
    return clip;
}

std::vector<juce::MidiMessage> scheduledMessages(const CompiledMidiClip& clip,
                                                 const std::int64_t start, const int sampleCount,
                                                 const std::int64_t timelineDelaySamples = 0) {
    juce::MidiBuffer buffer;
    MidiScheduler::schedule({clip}, start, sampleCount, buffer, timelineDelaySamples);
    std::vector<juce::MidiMessage> messages;
    for (const auto metadata : buffer) messages.push_back(metadata.getMessage());
    return messages;
}

TEST(MidiSchedulerTest, CompilesAndSortsPreparedTimelineEvents) {
    MidiClip source = makeClip(false);
    CompiledMidiClip compiled;
    juce::String error;

    ASSERT_TRUE(MidiScheduler::compile(source, TimelineTimebase{10, 60.0}, 10.0, compiled, error));
    ASSERT_EQ(compiled.lengthSamples, 4);
    ASSERT_EQ(compiled.events.size(), 5u);
    EXPECT_TRUE(compiled.events[0].message.isNoteOn());
    EXPECT_TRUE(compiled.events[1].message.isController());
    EXPECT_TRUE(compiled.events[2].message.isPitchWheel());
    EXPECT_TRUE(compiled.events[3].message.isChannelPressure());
    EXPECT_TRUE(compiled.events[4].message.isNoteOff());
    EXPECT_EQ(compiled.events[4].sampleOffset, 4);
}

TEST(MidiSchedulerTest, EmitsClippedNoteOffAtClipBoundaryAfterSeek) {
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(
        MidiScheduler::compile(makeClip(false), TimelineTimebase{10, 60.0}, 10.0, compiled, error));

    const auto messages = scheduledMessages(compiled, 4, 1);

    ASSERT_EQ(messages.size(), 1u);
    EXPECT_TRUE(messages.front().isNoteOff());
}

TEST(MidiSchedulerTest, EmitsLoopBoundaryOffBeforeTheNextLoopOn) {
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(
        MidiScheduler::compile(makeClip(true), TimelineTimebase{10, 60.0}, 10.0, compiled, error));

    const auto messages = scheduledMessages(compiled, 4, 1);

    ASSERT_EQ(messages.size(), 2u);
    EXPECT_TRUE(messages[0].isNoteOff());
    EXPECT_TRUE(messages[1].isNoteOn());
}

TEST(MidiSchedulerTest, UsesSourceCoordinatesForDelayedTimelineMidi) {
    CompiledMidiClip compiled;
    compiled.lengthSamples = 8'192;
    compiled.events.push_back({3'072, 0, juce::MidiMessage::controllerEvent(1, 1, 96)});

    const auto messages = scheduledMessages(compiled, 4'096, 256, 1'024);

    ASSERT_EQ(messages.size(), 1u);
    EXPECT_TRUE(messages.front().isController());
}

TEST(MidiSchedulerTest, KeepsLoopBoundariesVisibleWhenDelayExceedsBlockSize) {
    CompiledMidiClip compiled;
    compiled.lengthSamples = 64;
    compiled.loop = true;
    compiled.events.push_back({0, 1, juce::MidiMessage::noteOn(1, 60, 0.8f)});
    compiled.events.push_back({64, 0, juce::MidiMessage::noteOff(1, 60)});

    const auto messages = scheduledMessages(compiled, 128, 32, 64);

    ASSERT_EQ(messages.size(), 2u);
    EXPECT_TRUE(messages[0].isNoteOff());
    EXPECT_TRUE(messages[1].isNoteOn());
}

TEST(MidiSchedulerTest, CalculatesDensityInsteadOfTotalClipEventCount) {
    CompiledMidiClip distributed;
    distributed.lengthSamples = 2'000;
    for (std::int64_t offset = 0; offset < 2'000; ++offset)
        distributed.events.push_back(
            {offset, 0, juce::MidiMessage::controllerEvent(1, 1, static_cast<int>(offset % 127))});

    CompiledMidiClip dense;
    dense.lengthSamples = 2'000;
    for (std::int64_t offset = 400; offset < 1'500; ++offset)
        dense.events.push_back(
            {offset, 0, juce::MidiMessage::controllerEvent(1, 1, static_cast<int>(offset % 127))});

    CompiledMidiClip burst;
    burst.lengthSamples = 2'000;
    for (int index = 0; index < 1'100; ++index)
        burst.events.push_back({400, 0, juce::MidiMessage::controllerEvent(1, 1, index % 127)});

    EXPECT_EQ(MidiScheduler::maximumEventsPerBlock({distributed}, 4), 4u);
    EXPECT_EQ(MidiScheduler::maximumEventsPerBlock({dense}, 256), 256u);
    EXPECT_EQ(MidiScheduler::maximumEventsPerBlock({burst}, 256), 1'100u);
}

TEST(MidiSchedulerTest, PreservesDenseBlocksAfterPrepareCapacityIsCalculated) {
    CompiledMidiClip compiled;
    compiled.startSample = 0;
    compiled.lengthSamples = 512;
    for (std::int64_t offset = 0; offset < 300; ++offset)
        compiled.events.push_back(
            {offset, 0, juce::MidiMessage::controllerEvent(1, 1, static_cast<int>(offset % 127))});

    juce::MidiBuffer buffer;
    const auto capacity = MidiScheduler::maximumEventsPerBlock({compiled}, 512);
    ASSERT_TRUE(MidiScheduler::prepareBuffer(buffer, capacity));
    MidiScheduler::schedule({compiled}, 0, 512, buffer);

    std::size_t eventCount = 0;
    for (const auto metadata : buffer) ++eventCount;
    EXPECT_EQ(eventCount, 300u);
}

}  // namespace
}  // namespace riffra
