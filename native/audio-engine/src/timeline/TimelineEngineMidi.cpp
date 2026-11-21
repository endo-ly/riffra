#include <algorithm>

#include "TimelineEngine.h"

namespace riffra {

RealtimeRequest TimelineEngine::setLiveMidiTarget(const juce::String& trackId,
                                                  juce::String& error) {
    std::uint32_t trackKey = 0;
    if (trackId.isNotEmpty()) {
        const auto valid = graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
            const auto isInstrumentTrack = [&trackId](const PreparedTimeline* graph) {
                const auto* track = graph != nullptr ? graph->findTrack(trackId) : nullptr;
                return track != nullptr && track->runtime->instrumentTrack;
            };
            if (!isInstrumentTrack(graphs.latestCommitted)) return false;
            trackKey = graphs.trackKeys.keyFor(trackId);
            return true;
        });
        if (!valid) {
            error = "Live MIDI target must be an Instrument Track.";
            return RealtimeRequest::rejected;
        }
    }
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::setLiveMidiTarget;
    command.trackKey = trackKey;
    if (!submit(command).has_value()) {
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    return RealtimeRequest::accepted;
}

std::uint16_t TimelineEngine::midiSourceIndex(const juce::String& deviceId) {
    return midiSources.indexFor(deviceId);
}

bool TimelineEngine::enqueueLiveMidi(const std::uint16_t sourceIndex,
                                     const juce::MidiMessage& message) noexcept {
    if (!realtimeFrame.read().armedInstrumentTrack) return false;
    LiveMidiEvent event;
    event.sourceIndex = sourceIndex;
    const auto size = message.getRawDataSize();
    if (size <= 0 || size > static_cast<int>(sizeof(event.bytes))) {
        liveMidiQueueDrops.fetch_add(1, std::memory_order_relaxed);
        return true;
    }
    std::copy_n(message.getRawData(), size, event.bytes);
    event.size = static_cast<std::uint8_t>(size);
    if (!liveMidi.tryPush(event)) liveMidiQueueDrops.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::routeLiveMidi(RealtimeState& state) noexcept {
    LiveMidiEvent event;
    while (liveMidi.tryPop(event)) {
        if (state.graph == nullptr) continue;
        const juce::MidiMessage message(event.bytes, event.size);
        for (auto& trackPtr : state.graph->tracks) {
            auto& track = *trackPtr;
            auto& runtime = *track.runtime;
            if (!runtime.instrumentTrack || !runtime.armed ||
                !ArrangementGraph::midiRouteMatches(runtime.midiSourceIndex, runtime.midiChannel,
                                                    event.sourceIndex, message.getChannel()))
                continue;
            if (runtime.hasLoadedInstrument()) {
                if (const auto transformed =
                        runtime.midiNoteTransform.transformNoteMessage(message))
                    (void)runtime.enqueueMidi(*transformed);
                else if (!message.isNoteOn() && !message.isNoteOff())
                    (void)runtime.enqueueMidi(message);
            }
            if (state.recordingPhase == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(track.id, event.sourceIndex, message,
                                                 state.audioClockSample);
            }
        }
    }
}

std::optional<std::uint32_t> TimelineEngine::playSurfaceTrackKey(const juce::String& trackId,
                                                                 juce::String& error) {
    if (trackId.isEmpty()) {
        error = "A target Instrument Track is required.";
        return std::nullopt;
    }
    return graphRegistry.access(
        [&](ControlGraphRegistry::State& graphs) -> std::optional<std::uint32_t> {
            const auto* track = graphs.latestCommitted != nullptr
                                    ? graphs.latestCommitted->findTrack(trackId)
                                    : nullptr;
            if (track == nullptr) {
                error = "The target Track is not available in the Arrangement Graph.";
                return std::nullopt;
            }
            if (!track->runtime->instrumentTrack || !track->runtime->hasLoadedInstrument()) {
                error = "The target Instrument Track has no loaded instrument.";
                return std::nullopt;
            }
            return track->runtime->key;
        });
}

RealtimeRequest TimelineEngine::enqueueTargetedMidi(const juce::String& trackId,
                                                    const juce::MidiMessage& message,
                                                    juce::String& error) {
    const auto trackKey = playSurfaceTrackKey(trackId, error);
    if (!trackKey.has_value()) return RealtimeRequest::rejected;
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::targetedMidi;
    command.trackKey = *trackKey;
    const auto size = message.getRawDataSize();
    // The sidecar contract limits Play Surface messages to three bytes.
    jassert(size > 0 && size <= static_cast<int>(sizeof(command.midiBytes)));
    std::copy_n(message.getRawData(), size, command.midiBytes);
    command.midiSize = static_cast<std::uint8_t>(size);
    if (!submit(command).has_value()) {
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    return RealtimeRequest::accepted;
}

void TimelineEngine::playTargetedMidi(RealtimeState& state,
                                      const RealtimeCommand& command) noexcept {
    auto* track = findTrackByKey(state, command.trackKey);
    if (track == nullptr || !track->runtime->hasLoadedInstrument()) return;
    const juce::MidiMessage message(command.midiBytes, command.midiSize);
    if (const auto transformed = track->runtime->midiNoteTransform.transformNoteMessage(message))
        (void)track->runtime->enqueueMidi(*transformed);
    else if (!message.isNoteOn() && !message.isNoteOff())
        (void)track->runtime->enqueueMidi(message);
    if (track->runtime->armed && state.recordingPhase == RecordingPhase::recording)
        recordingCapture->writeMidiTrack(track->id, playSurfaceSourceId, message,
                                         state.audioClockSample);
}

RealtimeRequest TimelineEngine::panicTargetedMidi(const juce::String& trackId,
                                                  juce::String& error) {
    const auto trackKey = playSurfaceTrackKey(trackId, error);
    if (!trackKey.has_value()) return RealtimeRequest::rejected;
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::panicTrack;
    command.trackKey = *trackKey;
    if (!submit(command).has_value()) {
        error = "The realtime command queue is full.";
        return RealtimeRequest::queueFull;
    }
    return RealtimeRequest::accepted;
}

}  // namespace riffra
