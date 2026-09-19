#include "InstrumentPreviewSession.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <utility>

#include "../instruments/SonalloyInstrumentRuntime.h"
#include "InstrumentPreviewContract.h"
#include "InstrumentPreviewTiming.h"

namespace riffra {

std::unique_ptr<InstrumentPreviewSession> InstrumentPreviewSession::create(
    const juce::String& definitionJson, const juce::String& definitionBaseDir,
    InstrumentPreviewSpec spec, const double sampleRate, const int blockSize, juce::String& error) {
    if (definitionJson.isEmpty() || definitionBaseDir.isEmpty() || !std::isfinite(sampleRate) ||
        sampleRate <= 0.0 || blockSize <= 0 || !instrument_preview::isValidTempo(spec.tempoBpm) ||
        !instrument_preview::isValidTicksPerBeat(spec.ticksPerBeat) ||
        !instrument_preview::isWithinDurationLimit(spec.tempoBpm, spec.ticksPerBeat,
                                                   spec.lengthTicks) ||
        !instrument_preview::isValidNumerator(spec.timeSignature.numerator) ||
        !instrument_preview::isValidDenominator(spec.timeSignature.denominator)) {
        error = "Built-in instrument preview specification is invalid.";
        return nullptr;
    }
    if (spec.notes.size() < instrument_preview::kMinimumNoteCount ||
        spec.notes.size() > instrument_preview::kMaximumNoteCount) {
        error = "Built-in instrument preview contains an invalid number of notes.";
        return nullptr;
    }
    std::uint64_t previousTick = 0;
    bool hasPreviousTick = false;
    for (const auto& note : spec.notes) {
        if (hasPreviousTick && note.tick < previousTick) {
            error = "Built-in instrument preview notes are not sorted by tick.";
            return nullptr;
        }
        previousTick = note.tick;
        hasPreviousTick = true;
        if (note.durationTicks == 0 || note.durationTicks > spec.lengthTicks ||
            note.tick >= spec.lengthTicks || note.tick > spec.lengthTicks - note.durationTicks ||
            note.note > instrument_preview::kMaximumMidiValue || note.velocity == 0 ||
            note.velocity > instrument_preview::kMaximumMidiValue) {
            error = "Built-in instrument preview contains an invalid note.";
            return nullptr;
        }
    }

    auto runtime = SonalloyInstrumentRuntime::create(definitionJson, definitionBaseDir, sampleRate,
                                                     blockSize, error);
    if (runtime == nullptr) return nullptr;

    std::vector<ScheduledEvent> events;
    events.reserve(spec.notes.size() * 2);
    for (const auto& note : spec.notes) {
        events.push_back(ScheduledEvent{tickToFrame(note.tick, spec, sampleRate), true, note.note,
                                        note.velocity});
        events.push_back(ScheduledEvent{
            tickToFrame(note.tick + note.durationTicks, spec, sampleRate), false, note.note, 0});
    }
    std::stable_sort(events.begin(), events.end(), [](const auto& left, const auto& right) {
        if (left.frame != right.frame) return left.frame < right.frame;
        if (left.noteOn != right.noteOn) return !left.noteOn;
        return left.note < right.note;
    });
    std::size_t maximumEventsPerBlock = 0;
    std::uint64_t currentBlock = std::numeric_limits<std::uint64_t>::max();
    std::size_t currentBlockEvents = 0;
    for (const auto& event : events) {
        const auto eventBlock = event.frame / static_cast<std::uint64_t>(blockSize);
        if (eventBlock != currentBlock) {
            currentBlock = eventBlock;
            currentBlockEvents = 0;
        }
        maximumEventsPerBlock = std::max(maximumEventsPerBlock, ++currentBlockEvents);
    }
    if (!runtime->prepareTimelineMidiCapacity(maximumEventsPerBlock, error)) return nullptr;

    const auto lengthFrames = tickToFrame(spec.lengthTicks, spec, sampleRate);
    auto session = std::unique_ptr<InstrumentPreviewSession>(new InstrumentPreviewSession(
        std::unique_ptr<InstrumentRuntime>(std::move(runtime)), std::move(spec), sampleRate,
        blockSize, std::move(events), std::max<std::uint64_t>(1, lengthFrames)));
    session->midi.ensureSize(session->events.size() * 16 + 16);
    session->renderBuffer.setSize(2, blockSize);
    session->renderBuffer.clear();
    session->tailFrames = std::max(0, session->runtime->tailSamples());
    return session;
}

InstrumentPreviewSession::InstrumentPreviewSession(std::unique_ptr<InstrumentRuntime> runtimeIn,
                                                   InstrumentPreviewSpec specIn,
                                                   const double sampleRate, const int blockSizeIn,
                                                   std::vector<ScheduledEvent> eventsIn,
                                                   const std::uint64_t lengthFramesIn) noexcept
    : runtime(std::move(runtimeIn)),
      spec(std::move(specIn)),
      events(std::move(eventsIn)),
      lengthFrames(lengthFramesIn),
      blockSize(blockSizeIn) {
    juce::ignoreUnused(sampleRate);
}

InstrumentPreviewSession::~InstrumentPreviewSession() = default;

std::uint64_t InstrumentPreviewSession::tickToFrame(const std::uint64_t tick,
                                                    const InstrumentPreviewSpec& spec,
                                                    const double sampleRate) noexcept {
    const auto frames = static_cast<long double>(tick) * 60.0L * sampleRate /
                        (static_cast<long double>(spec.ticksPerBeat) * spec.tempoBpm);
    if (!std::isfinite(static_cast<double>(frames)) || frames <= 0.0L) return 0;
    const auto maxFrame = static_cast<long double>(std::numeric_limits<std::uint64_t>::max());
    if (frames >= maxFrame - 0.5L) return std::numeric_limits<std::uint64_t>::max();
    return static_cast<std::uint64_t>(std::floor(frames + 0.5L));
}

void InstrumentPreviewSession::process(float* const* outputChannels, const int outputChannelCount,
                                       const int numSamples, const double sampleRate) noexcept {
    if (!active.load(std::memory_order_acquire) || runtime == nullptr || numSamples <= 0 ||
        numSamples > blockSize || sampleRate <= 0.0) {
        return;
    }

    midi.clear();
    const auto sampleCount = static_cast<std::uint64_t>(numSamples);
    const auto maxFrame = std::numeric_limits<std::uint64_t>::max();
    const auto blockEnd =
        renderedFrames > maxFrame - sampleCount ? maxFrame : renderedFrames + sampleCount;
    while (nextEvent < events.size() && events[nextEvent].frame < blockEnd) {
        const auto& event = events[nextEvent];
        if (event.frame >= renderedFrames) {
            const auto offset = static_cast<int>(event.frame - renderedFrames);
            const auto message = event.noteOn
                                     ? juce::MidiMessage::noteOn(1, event.note, event.velocity)
                                     : juce::MidiMessage::noteOff(1, event.note);
            midi.addEvent(message, juce::jlimit(0, numSamples - 1, offset));
        }
        ++nextEvent;
    }

    renderBuffer.clear(0, numSamples);
    std::array<float*, 2> renderChannels{renderBuffer.getWritePointer(0),
                                         renderBuffer.getWritePointer(1)};
    const auto beatPosition =
        static_cast<double>(renderedFrames) / sampleRate * spec.tempoBpm / 60.0;
    const InstrumentProcessContext context{
        renderedFrames,
        spec.tempoBpm,
        beatPosition,
        instrument_preview::barPositionForFrame(renderedFrames, sampleRate, spec.tempoBpm,
                                                spec.timeSignature.numerator,
                                                spec.timeSignature.denominator),
        spec.timeSignature.numerator,
        spec.timeSignature.denominator,
        true,
    };
    runtime->process(renderChannels.data(), 2, numSamples, &midi, context);
    if (runtime->faultCode() != 0) {
        faultCode = runtime->faultCode();
        active.store(false, std::memory_order_release);
        return;
    }

    if (outputChannels != nullptr) {
        for (int channel = 0; channel < outputChannelCount; ++channel) {
            auto* output = outputChannels[channel];
            if (output == nullptr) continue;
            const auto* rendered = renderBuffer.getReadPointer(juce::jmin(channel, 1));
            juce::FloatVectorOperations::add(output, rendered, numSamples);
        }
    }
    renderedFrames = blockEnd;
    if (renderedFrames >= lengthFrames && nextEvent >= events.size()) {
        if (tailFrames <= 0)
            active.store(false, std::memory_order_release);
        else
            tailFrames = std::max(0, tailFrames - numSamples);
    }
}

void InstrumentPreviewSession::allNotesOff() noexcept {
    if (runtime != nullptr) runtime->allNotesOff();
}

}  // namespace riffra
