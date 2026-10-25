#include <gtest/gtest.h>

#include <array>
#include <vector>

#include "timeline/MidiScheduler.h"

namespace riffra {
namespace {

MidiClipSpec makeClip() {
    MidiClipSpec clip;
    clip.durationTicks = 4;
    clip.notes.push_back({0, 4, 60, 100, 1});
    clip.events.push_back({MidiEventKindSpec::controlChange, 1, 1, 7, 100});
    clip.events.push_back({MidiEventKindSpec::pitchBend, 2, 1, 0, 64});
    clip.events.push_back({MidiEventKindSpec::channelPressure, 3, 1, 80, 0});
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
    MidiClipSpec source = makeClip();
    MidiCompileOptions options;
    CompiledMidiClip compiled;
    juce::String error;

    ASSERT_TRUE(MidiScheduler::compile(
        source, options, TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0, compiled, error));
    ASSERT_EQ(compiled.lengthSamples, 4);
    ASSERT_EQ(compiled.events.size(), 5u);
    EXPECT_TRUE(compiled.events[0].message.isNoteOn());
    EXPECT_TRUE(compiled.events[1].message.isController());
    EXPECT_TRUE(compiled.events[2].message.isPitchWheel());
    EXPECT_TRUE(compiled.events[3].message.isChannelPressure());
    EXPECT_TRUE(compiled.events[4].message.isNoteOff());
    EXPECT_EQ(compiled.events[4].sampleOffset, 4);
}

TEST(MidiSchedulerTest, PreparedNoteMappingIsSharedByNormalAndInternalInstrumentEvents) {
    InstrumentMidiSpec settings;
    settings.transposeSemitones = 12;
    settings.playableNotes = PlayableNotesSpec{PlayableNotesKindSpec::range, 60, 96, {}};
    settings.keyswitches.push_back({24, "Sustain"});
    const MidiNoteTransform transform(settings);
    MidiClipSpec source;
    MidiCompileOptions options;
    source.durationTicks = 960;
    source.notes = {{0, 120, 60, 100, 1}, {240, 120, 24, 100, 1}, {480, 120, 120, 100, 1}};
    const TimelineTimebase timebase{10, {{0, 60.0}}, {{0, 4, 4}}};

    CompiledMidiClip midi;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(source, options, timebase, 10.0, midi, error, &transform))
        << error.toStdString();
    std::vector<int> midiNotes;
    for (const auto& event : midi.events)
        if (event.message.isNoteOn() || event.message.isNoteOff())
            midiNotes.push_back(event.message.getNoteNumber());
    EXPECT_EQ(midiNotes, (std::vector<int>{72, 72, 24, 24}));

    options.directInstrumentEvents = true;
    CompiledMidiClip internal;
    ASSERT_TRUE(
        MidiScheduler::compile(source, options, timebase, 10.0, internal, error, &transform))
        << error.toStdString();
    std::vector<int> internalNotes;
    for (const auto& event : internal.events)
        if (event.instrumentEvent && (event.instrumentEvent->event_type == SONALLOY_EVENT_NOTE_ON ||
                                      event.instrumentEvent->event_type == SONALLOY_EVENT_NOTE_OFF))
            internalNotes.push_back(event.instrumentEvent->note_number);
    EXPECT_EQ(internalNotes, (std::vector<int>{72, 72, 24, 24}));
}

TEST(MidiSchedulerTest, EmitsClippedNoteOffAtClipBoundaryAfterSeek) {
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(
        makeClip(), {}, TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0, compiled, error));

    const auto messages = scheduledMessages(compiled, 4, 1);

    ASSERT_EQ(messages.size(), 1u);
    EXPECT_TRUE(messages.front().isNoteOff());
}

TEST(MidiSchedulerTest, EmitsLoopBoundaryOffBeforeTheNextLoopOn) {
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(makeClip(), {.loop = true},
                                       TimelineTimebase{10, {{0, 60.0}}, {{0, 4, 4}}}, 10.0,
                                       compiled, error));

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
    MidiClipSpec source;
    MidiCompileOptions options;
    source.durationTicks = 3840;
    options.directInstrumentEvents = true;
    source.notes = {{480, 1440, 60, 90, 2}, {960, 1440, 60, 100, 2}};
    source.instrumentControlEvents = {{"bend", 1920, 2, "pitchBend", false, 0.12345f, {}},
                                      {"sustain", 3840, 3, "sustainPedal", false, 0.0f, {}}};
    TimelineTimebase timebase{960, {{0, 120.0}, {1920, 90.0}}, {{0, 4, 4}, {1920, 3, 4}}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(source, options, timebase, 48000.0, compiled, error))
        << error;
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
    MidiClipSpec source;
    MidiCompileOptions options;
    source.durationTicks = 960;
    options.directInstrumentEvents = true;
    options.loop = true;
    source.notes = {{0, 960, 60, 100, 1}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(
        MidiScheduler::compile(source, options, TimelineTimebase{}, 48000.0, compiled, error));
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

TEST(MidiSchedulerTest, ParameterRampUsesVariablePpqTempoMapAndRestoresSeekState) {
    MidiClipSpec source;
    MidiCompileOptions options;
    source.durationTicks = 13440;
    options.directInstrumentEvents = true;
    source.instrumentControlEvents = {
        {"ramp", 0, 0, "parameterRamp", false, 0.25f, "tone", 0.75f, 6720}};
    TimelineTimebase timebase{3360, {{0, 120.0}, {3360, 60.0}}, {{0, 4, 4}}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(MidiScheduler::compile(source, options, timebase, 48000.0, compiled, error))
        << error;
    ASSERT_EQ(compiled.events.size(), 1u);
    EXPECT_EQ(compiled.events[0].instrumentEvent->duration_frames, 72000u);
    std::array<SonalloyEvent, 4> events{};
    std::array<InstrumentEventOrder, 4> ordering{};
    auto count = MidiScheduler::scheduleInstrumentEvents({compiled}, 36000, 256, events.data(),
                                                         ordering.data(), events.size(), true);
    ASSERT_EQ(count, 1u);
    EXPECT_EQ(events[0].event_type, SONALLOY_EVENT_PARAMETER_RAMP);
    EXPECT_FLOAT_EQ(events[0].value, 0.5f);
    EXPECT_FLOAT_EQ(events[0].target_value, 0.75f);
    EXPECT_EQ(events[0].duration_frames, 36000u);
    count = MidiScheduler::scheduleInstrumentEvents({compiled}, 80000, 256, events.data(),
                                                    ordering.data(), events.size(), true);
    ASSERT_EQ(count, 1u);
    EXPECT_EQ(events[0].event_type, SONALLOY_EVENT_PARAMETER_CHANGE);
    EXPECT_FLOAT_EQ(events[0].value, 0.75f);
}

TEST(MidiSchedulerTest, RestoresLoopControlsFromPreviousIterationAndContinuesCurrentRamp) {
    MidiClipSpec source;
    MidiCompileOptions options;
    source.durationTicks = 1920;
    options.loop = true;
    options.directInstrumentEvents = true;
    source.instrumentControlEvents = {
        {"ramp", 1536, 0, "parameterRamp", false, 0.2f, "tone", 0.8f, 192},
        {"change", 1536, 1, "parameterChange", false, 0.8f, "level"}};
    CompiledMidiClip compiled;
    juce::String error;
    ASSERT_TRUE(
        MidiScheduler::compile(source, options, TimelineTimebase{}, 48000.0, compiled, error))
        << error;
    for (auto& event : compiled.events)
        event.instrumentEvent->parameter_handle = event.parameter == "tone" ? 7 : 8;
    std::array<SonalloyEvent, 4> events{};
    std::array<InstrumentEventOrder, 4> ordering{};

    EXPECT_EQ(MidiScheduler::scheduleInstrumentEvents({compiled}, 9600, 256, events.data(),
                                                      ordering.data(), events.size(), true),
              0u);
    for (const auto sample : {48000, 57600, 88800}) {
        const auto count = MidiScheduler::scheduleInstrumentEvents(
            {compiled}, sample, 256, events.data(), ordering.data(), events.size(), true);
        ASSERT_EQ(count, 2u);
        const auto ramp =
            std::find_if(events.begin(), events.begin() + count,
                         [](const auto& event) { return event.parameter_handle == 7; });
        const auto change =
            std::find_if(events.begin(), events.begin() + count,
                         [](const auto& event) { return event.parameter_handle == 8; });
        ASSERT_NE(ramp, events.begin() + count);
        ASSERT_NE(change, events.begin() + count);
        EXPECT_FLOAT_EQ(change->value, 0.8f);
        if (sample == 88800) {
            EXPECT_EQ(ramp->event_type, SONALLOY_EVENT_PARAMETER_RAMP);
            EXPECT_FLOAT_EQ(ramp->value, 0.5f);
            EXPECT_EQ(ramp->duration_frames, 2400u);
        } else {
            EXPECT_EQ(ramp->event_type, SONALLOY_EVENT_PARAMETER_CHANGE);
            EXPECT_FLOAT_EQ(ramp->value, 0.8f);
        }
    }
}

TEST(MidiSchedulerTest, RestoresLatestParameterAcrossClipsByTimePriorityAndSourceOrder) {
    const auto clip = [](const std::int64_t start, const int priority,
                         const std::uint32_t sourceOrder, const float value) {
        CompiledMidiClip result;
        result.startSample = start;
        result.startTick = static_cast<std::uint64_t>(start);
        result.durationTicks = 100;
        result.lengthSamples = 100;
        SonalloyEvent control{};
        control.event_type = SONALLOY_EVENT_PARAMETER_CHANGE;
        control.parameter_handle = 7;
        control.value = value;
        result.events.push_back({0, priority, {}, 0, sourceOrder, control, {}});
        return result;
    };
    const auto old = clip(100, 2, 9, 0.2f);
    const auto recent = clip(200, 2, 0, 0.8f);
    const auto highPriority = clip(200, 3, 0, 0.7f);
    const auto lastSource = clip(200, 3, 1, 0.9f);
    std::array<SonalloyEvent, 4> events{};
    std::array<InstrumentEventOrder, 4> ordering{};

    for (auto clips : {std::vector{recent, old}, std::vector{old, recent}}) {
        auto count = MidiScheduler::scheduleInstrumentEvents(clips, 300, 16, events.data(),
                                                             ordering.data(), events.size(), true);
        ASSERT_EQ(count, 1u);
        EXPECT_FLOAT_EQ(events[0].value, 0.8f);
        clips.insert(clips.begin(), lastSource);
        clips.push_back(highPriority);
        count = MidiScheduler::scheduleInstrumentEvents(clips, 300, 16, events.data(),
                                                        ordering.data(), events.size(), true);
        ASSERT_EQ(count, 1u);
        EXPECT_FLOAT_EQ(events[0].value, 0.9f);
    }
    const std::vector tied{recent, clip(200, 2, 0, 0.6f)};
    ASSERT_EQ(MidiScheduler::scheduleInstrumentEvents(tied, 200, 1, events.data(), ordering.data(),
                                                      events.size()),
              2u);
    EXPECT_FLOAT_EQ(events[1].value, 0.6f);
    ASSERT_EQ(MidiScheduler::scheduleInstrumentEvents(tied, 300, 1, events.data(), ordering.data(),
                                                      events.size(), true),
              1u);
    EXPECT_FLOAT_EQ(events[0].value, 0.6f);
}

}  // namespace
}  // namespace riffra
