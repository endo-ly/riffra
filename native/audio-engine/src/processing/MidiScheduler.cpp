#include "MidiScheduler.h"

#include <algorithm>

namespace riffra {
namespace {

int channel(const int value) noexcept { return juce::jlimit(1, 16, value); }

void appendEvent(CompiledMidiClip& destination, const juce::MidiMessage& message,
                 const std::int64_t offset, const int ordering) {
    if (offset < 0 || offset > destination.lengthSamples) return;
    destination.events.push_back({offset, ordering, message});
}

}  // namespace

bool MidiScheduler::compile(const MidiClip& source, const TimelineTimebase& timebase,
                            const double sampleRate, CompiledMidiClip& destination,
                            juce::String& error) {
    destination = {};
    destination.startSample = timebase.tickToSample(source.startTick, sampleRate);
    destination.lengthSamples =
        std::max<std::int64_t>(1, timebase.tickToSample(source.durationTicks, sampleRate));
    destination.loop = source.loop;
    destination.muted = source.muted;
    for (const auto& note : source.notes) {
        if (note.durationTicks == 0 || note.startTick >= source.durationTicks || note.note < 0 ||
            note.note > 127) {
            error = "Timeline MIDI note has an invalid musical range.";
            return false;
        }
        const auto noteStart = std::min<std::int64_t>(
            destination.lengthSamples - 1,
            std::max<std::int64_t>(0, timebase.tickToSample(note.startTick, sampleRate)));
        const auto noteEnd = std::min<std::int64_t>(
            destination.lengthSamples,
            noteStart +
                std::max<std::int64_t>(1, timebase.tickToSample(note.durationTicks, sampleRate)));
        appendEvent(destination,
                    juce::MidiMessage::noteOn(
                        channel(note.channel), note.note,
                        static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))),
                    noteStart, 1);
        appendEvent(destination, juce::MidiMessage::noteOff(channel(note.channel), note.note),
                    noteEnd, 0);
    }
    for (const auto& event : source.events) {
        if (event.tick >= source.durationTicks ||
            (event.kind != "controlChange" && event.kind != "pitchBend" &&
             event.kind != "channelPressure")) {
            error = "Timeline MIDI event has an invalid type or musical position.";
            return false;
        }
        const auto offset = std::min<std::int64_t>(
            destination.lengthSamples - 1,
            std::max<std::int64_t>(0, timebase.tickToSample(event.tick, sampleRate)));
        const auto eventChannel = channel(event.channel);
        if (event.kind == "controlChange")
            appendEvent(destination,
                        juce::MidiMessage::controllerEvent(eventChannel, event.data1, event.data2),
                        offset, 2);
        else if (event.kind == "pitchBend")
            appendEvent(
                destination,
                juce::MidiMessage::pitchWheel(eventChannel, event.data1 | (event.data2 << 7)),
                offset, 2);
        else
            appendEvent(destination,
                        juce::MidiMessage::channelPressureChange(eventChannel, event.data1), offset,
                        2);
    }
    std::stable_sort(destination.events.begin(), destination.events.end(),
                     [](const auto& left, const auto& right) {
                         if (left.sampleOffset != right.sampleOffset)
                             return left.sampleOffset < right.sampleOffset;
                         return left.ordering < right.ordering;
                     });
    return true;
}

void MidiScheduler::schedule(const std::vector<CompiledMidiClip>& clips,
                             const std::int64_t rangeStart, const int sampleCount,
                             juce::MidiBuffer& destination) noexcept {
    if (sampleCount <= 0) return;
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : clips) {
        if (clip.muted || clip.lengthSamples <= 0) continue;
        const auto firstIteration =
            clip.loop && rangeStart > clip.startSample
                ? std::max<std::int64_t>(0,
                                         (rangeStart - clip.startSample) / clip.lengthSamples - 1)
                : 0;
        const auto lastIteration =
            clip.loop ? std::max<std::int64_t>(
                            firstIteration, (rangeEnd - clip.startSample + clip.lengthSamples - 1) /
                                                clip.lengthSamples)
                      : 0;
        for (std::int64_t iteration = firstIteration; iteration <= lastIteration; ++iteration) {
            const auto iterationStart = clip.startSample + iteration * clip.lengthSamples;
            const auto localStart = std::max<std::int64_t>(0, rangeStart - iterationStart);
            const auto localEnd = std::min<std::int64_t>(
                clip.lengthSamples, std::max<std::int64_t>(0, rangeEnd - iterationStart));
            const auto isClipBoundary =
                localStart == clip.lengthSamples && localEnd == clip.lengthSamples;
            if (localEnd < localStart || (localEnd == localStart && !isClipBoundary)) {
                if (!clip.loop) break;
                continue;
            }
            const auto begin =
                std::lower_bound(clip.events.begin(), clip.events.end(), localStart,
                                 [](const CompiledMidiEvent& event, const std::int64_t value) {
                                     return event.sampleOffset < value;
                                 });
            for (auto event = begin; event != clip.events.end(); ++event) {
                const auto inRange =
                    event->sampleOffset < localEnd ||
                    (event->sampleOffset == clip.lengthSamples && localEnd == clip.lengthSamples);
                if (!inRange) break;
                const auto absoluteSample = iterationStart + event->sampleOffset;
                const auto offset = static_cast<int>(absoluteSample - rangeStart);
                if (offset >= 0 && offset < sampleCount)
                    destination.addEvent(event->message, offset);
            }
            if (!clip.loop) break;
        }
    }
}

}  // namespace riffra
