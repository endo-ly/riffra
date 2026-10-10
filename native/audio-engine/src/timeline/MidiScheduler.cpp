#include "MidiScheduler.h"

#include <algorithm>
#include <limits>
#include <numeric>
#include <tuple>

namespace riffra {
namespace {

int channel(const int value) noexcept { return juce::jlimit(1, 16, value); }

void appendEvent(CompiledMidiClip& destination, const juce::MidiMessage& message,
                 const std::int64_t offset, const int ordering) {
    if (offset < 0 || offset > destination.lengthSamples) return;
    destination.events.push_back({offset, ordering, message});
}

std::size_t saturatingAdd(const std::size_t left, const std::size_t right) noexcept {
    if (right > std::numeric_limits<std::size_t>::max() - left)
        return std::numeric_limits<std::size_t>::max();
    return left + right;
}

std::size_t saturatingMultiply(const std::size_t left, const std::size_t right) noexcept {
    if (left != 0 && right > std::numeric_limits<std::size_t>::max() / left)
        return std::numeric_limits<std::size_t>::max();
    return left * right;
}

std::size_t maximumEventsInWindow(std::vector<std::int64_t>& eventSamples,
                                  const int blockSize) noexcept {
    if (eventSamples.empty()) return 0;
    std::sort(eventSamples.begin(), eventSamples.end());
    std::size_t maximum = 0;
    std::size_t firstInWindow = 0;
    for (std::size_t lastInWindow = 0; lastInWindow < eventSamples.size(); ++lastInWindow) {
        while (firstInWindow <= lastInWindow &&
               eventSamples[lastInWindow] - eventSamples[firstInWindow] >= blockSize)
            ++firstInWindow;
        maximum = std::max(maximum, lastInWindow - firstInWindow + 1);
    }
    return maximum;
}

std::size_t maximumLoopEvents(const CompiledMidiClip& clip, const int blockSize) noexcept {
    const auto eventCount = clip.events.size();
    const auto loopLength = clip.lengthSamples;
    const auto blockLength = static_cast<std::int64_t>(blockSize);
    const auto completeLoops = blockLength / loopLength;
    const auto remainder = blockLength % loopLength;

    // Events at a loop endpoint and at the next loop's offset zero share a
    // timestamp. Treat endpoint events as phase zero when evaluating the
    // remainder so the boundary is counted without expanding the timeline.
    std::size_t zeroOffsetCount = 0;
    while (zeroOffsetCount < eventCount && clip.events[zeroOffsetCount].sampleOffset == 0)
        ++zeroOffsetCount;
    std::size_t endpointCount = 0;
    while (endpointCount < eventCount &&
           clip.events[eventCount - endpointCount - 1].sampleOffset == loopLength)
        ++endpointCount;

    const auto phaseAt = [&](const std::size_t index,
                             const std::int64_t loopIndex) noexcept -> std::int64_t {
        std::int64_t phase = 0;
        if (index < zeroOffsetCount) {
            phase = clip.events[index].sampleOffset;
        } else if (index < zeroOffsetCount + endpointCount) {
            phase = 0;
        } else {
            phase =
                clip.events[zeroOffsetCount + index - zeroOffsetCount - endpointCount].sampleOffset;
        }
        return phase + loopIndex * loopLength;
    };

    const auto completeLoopEvents =
        saturatingMultiply(static_cast<std::size_t>(completeLoops), eventCount);
    if (remainder == 0 || completeLoopEvents == std::numeric_limits<std::size_t>::max())
        return completeLoopEvents;

    std::size_t maximumRemainderEvents = 0;
    std::size_t firstInWindow = 0;
    for (std::size_t lastInWindow = 0; lastInWindow < eventCount * 2; ++lastInWindow) {
        const auto lastLoop = static_cast<std::int64_t>(lastInWindow / eventCount);
        const auto lastPhase = phaseAt(lastInWindow % eventCount, lastLoop);
        while (firstInWindow <= lastInWindow) {
            const auto firstLoop = static_cast<std::int64_t>(firstInWindow / eventCount);
            const auto firstPhase = phaseAt(firstInWindow % eventCount, firstLoop);
            if (lastPhase - firstPhase < remainder) break;
            ++firstInWindow;
        }
        maximumRemainderEvents = std::max(maximumRemainderEvents, lastInWindow - firstInWindow + 1);
    }
    return saturatingAdd(completeLoopEvents, maximumRemainderEvents);
}

std::int64_t positiveModulo(const std::int64_t value, const std::int64_t modulus) noexcept {
    const auto remainder = value % modulus;
    return remainder >= 0 ? remainder : remainder + modulus;
}

std::int64_t addModulo(const std::int64_t left, const std::int64_t right,
                       const std::int64_t modulus) noexcept {
    const auto offset = positiveModulo(right, modulus);
    if (offset == 0) return left;
    return left >= modulus - offset ? left - (modulus - offset) : left + offset;
}

std::size_t maximumPeriodicEvents(const std::vector<const CompiledMidiClip*>& clips,
                                  const int blockSize) noexcept {
    constexpr std::size_t kMaximumAnalysisEvents = 1'000'000;
    std::int64_t period = 1;
    for (const auto* clip : clips) {
        if (clip->lengthSamples <= 0 || clip->events.empty()) continue;
        const auto divisor = std::gcd(period, clip->lengthSamples);
        const auto reduced = period / divisor;
        if (reduced > std::numeric_limits<std::int64_t>::max() / clip->lengthSamples)
            return std::numeric_limits<std::size_t>::max();
        period = reduced * clip->lengthSamples;
    }
    if (period > std::numeric_limits<std::int64_t>::max() / 2)
        return std::numeric_limits<std::size_t>::max();

    std::size_t eventCount = 0;
    for (const auto* clip : clips) {
        if (clip->lengthSamples <= 0 || clip->events.empty()) continue;
        const auto repetitions = static_cast<std::size_t>(period / clip->lengthSamples);
        if (repetitions > kMaximumAnalysisEvents ||
            clip->events.size() > kMaximumAnalysisEvents / repetitions ||
            eventCount > kMaximumAnalysisEvents - repetitions * clip->events.size())
            return std::numeric_limits<std::size_t>::max();
        eventCount += repetitions * clip->events.size();
    }
    if (eventCount == 0) return 0;

    std::vector<std::int64_t> phases;
    phases.reserve(eventCount);
    for (const auto* clip : clips) {
        if (clip->lengthSamples <= 0 || clip->events.empty()) continue;
        const auto repetitions = period / clip->lengthSamples;
        const auto clipStart = positiveModulo(clip->startSample, period);
        for (std::int64_t iteration = 0; iteration < repetitions; ++iteration) {
            const auto iterationStart =
                addModulo(clipStart, iteration * clip->lengthSamples, period);
            for (const auto& event : clip->events)
                phases.push_back(addModulo(iterationStart, event.sampleOffset, period));
        }
    }
    std::sort(phases.begin(), phases.end());

    const auto periodLength = period;
    const auto blockLength = static_cast<std::int64_t>(blockSize);
    const auto fullPeriods = blockLength / periodLength;
    const auto remainder = blockLength % periodLength;
    const auto fullPeriodEvents =
        saturatingMultiply(static_cast<std::size_t>(fullPeriods), phases.size());
    if (fullPeriodEvents == std::numeric_limits<std::size_t>::max()) return fullPeriodEvents;
    if (remainder == 0) return fullPeriodEvents;

    std::vector<std::int64_t> repeatedPhases;
    repeatedPhases.reserve(phases.size() * 2);
    repeatedPhases.insert(repeatedPhases.end(), phases.begin(), phases.end());
    for (const auto phase : phases) repeatedPhases.push_back(phase + periodLength);
    const auto remainderMaximum =
        maximumEventsInWindow(repeatedPhases, static_cast<int>(remainder));
    return saturatingAdd(fullPeriodEvents, remainderMaximum);
}

}  // namespace

std::size_t MidiScheduler::maximumEventsPerBlock(const std::vector<CompiledMidiClip>& clips,
                                                 const int blockSize) noexcept {
    if (blockSize <= 0) return 0;
    std::vector<std::int64_t> nonLoopEventSamples;
    std::vector<const CompiledMidiClip*> loopClips;
    for (const auto& clip : clips) {
        if (clip.muted || clip.events.empty() || clip.lengthSamples <= 0) continue;
        if (clip.loop) {
            loopClips.push_back(&clip);
            continue;
        }
        if (clip.events.size() >
            std::numeric_limits<std::size_t>::max() - nonLoopEventSamples.size())
            return std::numeric_limits<std::size_t>::max();
        nonLoopEventSamples.reserve(nonLoopEventSamples.size() + clip.events.size());
        for (const auto& event : clip.events)
            nonLoopEventSamples.push_back(clip.startSample + event.sampleOffset);
    }
    auto loopMaximum = maximumPeriodicEvents(loopClips, blockSize);
    if (loopMaximum == std::numeric_limits<std::size_t>::max()) {
        loopMaximum = 0;
        for (const auto* clip : loopClips) {
            loopMaximum = saturatingAdd(loopMaximum, maximumLoopEvents(*clip, blockSize));
            if (loopMaximum == std::numeric_limits<std::size_t>::max()) return loopMaximum;
        }
    }
    return saturatingAdd(maximumEventsInWindow(nonLoopEventSamples, blockSize), loopMaximum);
}

bool MidiScheduler::prepareBuffer(juce::MidiBuffer& buffer,
                                  const std::size_t eventCapacity) noexcept {
    constexpr auto bytesPerEvent = kMaximumMessageBytes + kMidiEventOverhead;
    if (eventCapacity > static_cast<std::size_t>(std::numeric_limits<int>::max()) / bytesPerEvent)
        return false;
    buffer.ensureSize(static_cast<int>(eventCapacity * bytesPerEvent));
    return true;
}

bool MidiScheduler::compile(const MidiClip& source, const TimelineTimebase& timebase,
                            const double sampleRate, CompiledMidiClip& destination,
                            juce::String& error) {
    destination = {};
    destination.startTick = source.startTick;
    destination.durationTicks = source.durationTicks;
    destination.startSample = timebase.tickToSample(source.startTick, sampleRate);
    if (source.startTick > std::numeric_limits<std::uint64_t>::max() - source.durationTicks) {
        error = "Timeline MIDI clip end overflows the tick counter.";
        return false;
    }
    destination.lengthSamples = std::max<std::int64_t>(
        1, timebase.tickToSample(source.startTick + source.durationTicks, sampleRate) -
               destination.startSample);
    destination.loop = source.loop;
    destination.muted = source.muted;
    std::uint64_t noteId = source.noteIdBase;
    std::uint32_t noteOrder = 0;
    for (const auto& note : source.notes) {
        if (note.durationTicks == 0 || note.startTick >= source.durationTicks || note.note < 0 ||
            note.note > 127) {
            error = "Timeline MIDI note has an invalid musical range.";
            return false;
        }
        const auto noteStart = std::min<std::int64_t>(
            destination.lengthSamples - 1,
            std::max<std::int64_t>(
                0, timebase.tickToSample(source.startTick + note.startTick, sampleRate) -
                       destination.startSample));
        const auto noteEnd = std::min<std::int64_t>(
            destination.lengthSamples,
            std::max<std::int64_t>(
                noteStart + 1,
                timebase.tickToSample(
                    source.startTick + note.startTick +
                        std::min(note.durationTicks, source.durationTicks - note.startTick),
                    sampleRate) -
                    destination.startSample));
        if (source.directInstrumentEvents) {
            SonalloyEvent on{};
            on.event_type = SONALLOY_EVENT_NOTE_ON;
            on.note_id = noteId++;
            on.note_number = static_cast<std::uint8_t>(note.note);
            on.velocity = static_cast<std::uint8_t>(note.velocity);
            SonalloyEvent off{};
            off.event_type = SONALLOY_EVENT_NOTE_OFF;
            off.note_id = on.note_id;
            destination.events.push_back({noteStart, 6, {}, note.startTick, noteOrder, on, {}});
            destination.events.push_back(
                {noteEnd, 1, {}, note.startTick + note.durationTicks, noteOrder++, off, {}});
        } else {
            appendEvent(destination,
                        juce::MidiMessage::noteOn(
                            channel(note.channel), note.note,
                            static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))),
                        noteStart, 1);
            appendEvent(destination, juce::MidiMessage::noteOff(channel(note.channel), note.note),
                        noteEnd, 0);
        }
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
            std::max<std::int64_t>(
                0, timebase.tickToSample(source.startTick + event.tick, sampleRate) -
                       destination.startSample));
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
    if (!source.instrumentControlEvents.empty() && !source.directInstrumentEvents) {
        error = "Instrument controls require a Sonalloy instrument.";
        return false;
    }
    for (const auto& control : source.instrumentControlEvents) {
        SonalloyEvent event{};
        int priority = 0;
        event.value = control.value;
        if (control.type == "sustainPedal") {
            event.event_type = SONALLOY_EVENT_SUSTAIN;
            event.bool_value = control.down ? 1 : 0;
        } else if (control.type == "parameterChange") {
            event.event_type = SONALLOY_EVENT_PARAMETER_CHANGE;
            priority = 2;
        } else if (control.type == "pitchBend") {
            event.event_type = SONALLOY_EVENT_PITCH_BEND;
            priority = 3;
        } else if (control.type == "modWheel") {
            event.event_type = SONALLOY_EVENT_MOD_WHEEL;
            priority = 4;
        } else if (control.type == "aftertouch") {
            event.event_type = SONALLOY_EVENT_AFTERTOUCH;
            priority = 5;
        } else {
            error = "Unknown instrument control event.";
            return false;
        }
        const auto offset = timebase.tickToSample(source.startTick + control.tick, sampleRate) -
                            destination.startSample;
        destination.events.push_back(
            {offset, priority, {}, control.tick, control.sourceOrder, event, control.parameter});
    }
    std::stable_sort(destination.events.begin(), destination.events.end(),
                     [](const auto& left, const auto& right) {
                         if (left.sampleOffset != right.sampleOffset)
                             return left.sampleOffset < right.sampleOffset;
                         if (left.instrumentEvent && right.instrumentEvent &&
                             left.originalTick != right.originalTick)
                             return left.originalTick < right.originalTick;
                         if (left.ordering != right.ordering) return left.ordering < right.ordering;
                         return left.sourceOrder < right.sourceOrder;
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
                if (!event->instrumentEvent && offset >= 0 && offset < sampleCount)
                    (void)destination.addEvent(event->message, offset);
            }
            if (!clip.loop) break;
        }
    }
}

std::size_t MidiScheduler::scheduleInstrumentEvents(const std::vector<CompiledMidiClip>& clips,
                                                    const std::int64_t rangeStart,
                                                    const int sampleCount,
                                                    SonalloyEvent* destination,
                                                    InstrumentEventOrder* ordering,
                                                    const std::size_t capacity) noexcept {
    std::size_t count = 0;
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : clips) {
        if (clip.muted || clip.lengthSamples <= 0) continue;
        const auto first = clip.loop && rangeStart > clip.startSample
                               ? std::max<std::int64_t>(
                                     0, (rangeStart - clip.startSample) / clip.lengthSamples - 1)
                               : 0;
        const auto last = clip.loop ? std::max<std::int64_t>(
                                          first, (rangeEnd - clip.startSample) / clip.lengthSamples)
                                    : 0;
        for (auto iteration = first; iteration <= last; ++iteration) {
            const auto start = clip.startSample + iteration * clip.lengthSamples;
            const auto begin =
                std::lower_bound(clip.events.begin(), clip.events.end(), rangeStart - start,
                                 [](const CompiledMidiEvent& event, std::int64_t sample) {
                                     return event.sampleOffset < sample;
                                 });
            for (auto event = begin; event != clip.events.end(); ++event) {
                const auto sample = start + event->sampleOffset;
                if (sample >= rangeEnd) break;
                if (!event->instrumentEvent) continue;
                if (count == capacity) return capacity + 1;
                auto prepared = *event->instrumentEvent;
                prepared.sample_offset = static_cast<std::uint32_t>(sample - rangeStart);
                if (prepared.event_type == SONALLOY_EVENT_NOTE_ON ||
                    prepared.event_type == SONALLOY_EVENT_NOTE_OFF) {
                    constexpr auto maximumId = std::numeric_limits<std::uint64_t>::max() >> 1;
                    if (clip.noteIdStride != 0 &&
                        static_cast<std::uint64_t>(iteration) >
                            (maximumId - prepared.note_id) / clip.noteIdStride)
                        return capacity + 1;
                    prepared.note_id += static_cast<std::uint64_t>(iteration) * clip.noteIdStride;
                }
                const InstrumentEventOrder order{
                    clip.startTick + static_cast<std::uint64_t>(iteration) * clip.durationTicks +
                        event->originalTick,
                    event->ordering, event->sourceOrder};
                const auto key = std::tuple(prepared.sample_offset, order.tick, order.priority,
                                            order.sourceOrder);
                auto position = count++;
                while (position > 0 &&
                       std::tuple(destination[position - 1].sample_offset,
                                  ordering[position - 1].tick, ordering[position - 1].priority,
                                  ordering[position - 1].sourceOrder) > key) {
                    destination[position] = destination[position - 1];
                    ordering[position] = ordering[position - 1];
                    --position;
                }
                destination[position] = prepared;
                ordering[position] = order;
            }
        }
    }
    return count;
}

}  // namespace riffra
