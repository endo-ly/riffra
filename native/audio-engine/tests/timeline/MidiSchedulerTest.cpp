#include <gtest/gtest.h>

#include <vector>

#include "timeline/MidiScheduler.h"

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
                                                 const std::int64_t start, const int sampleCount) {
    juce::MidiBuffer buffer;
    MidiScheduler::schedule({clip}, start, sampleCount, buffer);
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

TEST(MidiSchedulerTest, CalculatesDensityAcrossSeparatedClipsUsingAbsoluteSamples) {
    std::vector<CompiledMidiClip> clips;
    for (int index = 0; index < 1'000; ++index) {
        CompiledMidiClip clip;
        clip.startSample = static_cast<std::int64_t>(index) * 512;
        clip.lengthSamples = 128;
        clip.events.push_back({0, 0, juce::MidiMessage::controllerEvent(1, 1, index % 127)});
        clip.events.push_back({1, 0, juce::MidiMessage::controllerEvent(1, 2, index % 127)});
        clips.push_back(std::move(clip));
    }

    EXPECT_EQ(MidiScheduler::maximumEventsPerBlock(clips, 256), 2u);
}

TEST(MidiSchedulerTest, CalculatesDensityAcrossLoopClipsUsingAbsolutePhase) {
    CompiledMidiClip first;
    first.lengthSamples = 16;
    first.loop = true;
    first.events.push_back({0, 0, juce::MidiMessage::controllerEvent(1, 1, 1)});

    CompiledMidiClip second;
    second.startSample = 8;
    second.lengthSamples = 16;
    second.loop = true;
    second.events.push_back({0, 0, juce::MidiMessage::controllerEvent(1, 1, 2)});

    EXPECT_EQ(MidiScheduler::maximumEventsPerBlock({first, second}, 4), 1u);
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
