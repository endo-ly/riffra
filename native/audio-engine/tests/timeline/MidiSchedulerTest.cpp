#include <gtest/gtest.h>

#include <array>
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

    ASSERT_TRUE(MidiScheduler::compile(source, TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0,
                                       compiled, error));
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
    ASSERT_TRUE(MidiScheduler::compile(
        makeClip(false), TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0, compiled, error));

    const auto messages = scheduledMessages(compiled, 4, 1);

    ASSERT_EQ(messages.size(), 1u);
    EXPECT_TRUE(messages.front().isNoteOff());
}

TEST(MidiSchedulerTest, EmitsLoopBoundaryOffBeforeTheNextLoopOn) {
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(
        makeClip(true), TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0, compiled, error));

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

TEST(MidiSchedulerTest, PreservesOverlappingNoteIdsPreciseControlsAndClipEndpoint) {
    MidiClip source;
    source.durationTicks = 3840;
    source.directInstrumentEvents = true;
    source.notes = {{480, 1440, 60, 90, 2}, {960, 1440, 60, 100, 2}};
    source.instrumentControlEvents = {{"bend", 1920, 2, "pitchBend", false, 0.12345f, {}},
                                      {"sustain", 3840, 3, "sustainPedal", false, 0.0f, {}}};
    TimelineTimebase timebase{960, {{0, 120.0}, {1920, 90.0}}, {{0, 4, 4}, {1920, 3, 4}}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(source, timebase, 48000.0, compiled, error)) << error;
    std::array<SonalloyEvent, 8> events{};
    std::array<InstrumentEventOrder, 8> ordering{};
    const auto count = MidiScheduler::scheduleInstrumentEvents({compiled}, 0, 112001, events.data(),
                                                               ordering.data(), events.size());
    ASSERT_EQ(count, 6u);
    EXPECT_EQ(events[0].note_id, 0u);
    EXPECT_EQ(events[1].note_id, 1u);
    EXPECT_EQ(events[2].event_type, SONALLOY_EVENT_NOTE_OFF);
    EXPECT_EQ(events[2].note_id, 0u);
    EXPECT_EQ(events[2].sample_offset, 48000u);
    EXPECT_FLOAT_EQ(events[3].value, 0.12345f);
    EXPECT_EQ(events[4].note_id, 1u);
    EXPECT_EQ(events[4].sample_offset, 64000u);
    EXPECT_EQ(events[5].event_type, SONALLOY_EVENT_SUSTAIN);
    EXPECT_EQ(events[5].sample_offset, 112000u);
}

TEST(MidiSchedulerTest, OrdersAdjacentClipsAndAllocatesDifferentLoopNoteIds) {
    MidiClip source;
    source.durationTicks = 960;
    source.directInstrumentEvents = true;
    source.loop = true;
    source.notes = {{0, 960, 60, 100, 1}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(source, TimelineTimebase{}, 48000.0, compiled, error));
    compiled.noteIdStride = 1;
    std::array<SonalloyEvent, 8> events{};
    std::array<InstrumentEventOrder, 8> ordering{};
    auto count = MidiScheduler::scheduleInstrumentEvents({compiled}, 24000, 1, events.data(),
                                                         ordering.data(), events.size());
    ASSERT_EQ(count, 2u);
    EXPECT_EQ(events[0].event_type, SONALLOY_EVENT_NOTE_OFF);
    EXPECT_EQ(events[0].note_id, 0u);
    EXPECT_EQ(events[1].event_type, SONALLOY_EVENT_NOTE_ON);
    EXPECT_EQ(events[1].note_id, 1u);
    auto next = compiled;
    next.loop = false;
    next.startSample = 24000;
    next.startTick = 960;
    compiled.loop = false;
    count = MidiScheduler::scheduleInstrumentEvents({next, compiled}, 24000, 1, events.data(),
                                                    ordering.data(), events.size());
    ASSERT_EQ(count, 2u);
    EXPECT_EQ(events[0].event_type, SONALLOY_EVENT_NOTE_OFF);
    EXPECT_EQ(events[1].event_type, SONALLOY_EVENT_NOTE_ON);
}

}  // namespace
}  // namespace riffra
