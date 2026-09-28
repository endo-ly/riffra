#include <algorithm>
#include <utility>

#include "TimelineEngine.h"

namespace riffra {

namespace {

using Track = PreparedTimeline::Track;

/// Finds the plugin rack of a Track device; `nullptr` for built-in instruments
/// and unknown devices.
PluginRack* findRack(Track& track, const juce::String& deviceId) noexcept {
    auto& runtime = *track.runtime;
    if (runtime.instrumentTrack && track.instrumentDeviceId == deviceId)
        return runtime.instrument() != nullptr ? runtime.instrument()->vst3Rack() : nullptr;
    return runtime.effects().findDevice(deviceId);
}

bool isInstrumentDevice(const Track& track, const juce::String& deviceId) noexcept {
    return track.runtime->instrumentTrack && track.instrumentDeviceId == deviceId;
}

}  // namespace

RealtimeRequest TimelineEngine::setLiveMidiTarget(const juce::String& trackId,
                                                  juce::String& error) {
    std::uint32_t trackKey = 0;
    if (trackId.isNotEmpty()) {
        const auto valid = graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
            const auto isInstrumentTrack = [&trackId](const PreparedTimeline* graph) {
                const auto* track = graph != nullptr ? graph->findTrack(trackId) : nullptr;
                return track != nullptr && track->runtime->instrumentTrack;
            };
            if (!isInstrumentTrack(graphs.latestCommitted) &&
                !isInstrumentTrack(graphs.pending.get()))
                return false;
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
            if (runtime.hasLoadedInstrument()) (void)runtime.enqueueMidi(message);
            if (state.recordingPhase == RecordingPhase::recording)
                recordingCapture->writeMidiTrack(track.id, midiSources.deviceId(event.sourceIndex),
                                                 message, state.audioClockSample);
        }
    }
}

PreparedTimeline::Track* TimelineEngine::findTrackByKey(const RealtimeState& state,
                                                        const std::uint32_t trackKey) noexcept {
    if (state.graph == nullptr) return nullptr;
    for (auto& track : state.graph->tracks)
        if (track->runtime->key == trackKey) return track.get();
    return nullptr;
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

void TimelineEngine::resetPluginDevices() noexcept {
    const auto frame = realtimeFrame.read();
    graphRegistry.access([&frame](ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.find(frame.activeGraphSerial);
        if (graph == nullptr) return;
        for (const auto& track : graph->tracks) {
            auto& runtime = *track->runtime;
            if (runtime.instrument() != nullptr)
                if (auto* rack = runtime.instrument()->vst3Rack()) rack->reset();
            runtime.effects().reset();
        }
    });
}

PluginRack* TimelineEngine::findDevice(const juce::String& trackId,
                                       const juce::String& deviceId) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) -> PluginRack* {
        auto* track = graphs.latestCommitted != nullptr ? graphs.latestCommitted->findTrack(trackId)
                                                        : nullptr;
        return track != nullptr ? findRack(*track, deviceId) : nullptr;
    });
}

const PluginRack* TimelineEngine::findCommittedRack(const ControlGraphRegistry::State& graphs,
                                                    const juce::String& trackId,
                                                    const juce::String& deviceId, const char* noun,
                                                    juce::String& error) {
    if (graphs.latestCommitted == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return nullptr;
    }
    auto* track = graphs.latestCommitted->findTrack(trackId);
    if (track == nullptr) {
        error = "Track was not found.";
        return nullptr;
    }
    const auto* rack = findRack(*track, deviceId);
    if (rack == nullptr)
        error = isInstrumentDevice(*track, deviceId)
                    ? juce::String("Built-in instruments do not expose plugin ") + noun + "."
                    : juce::String("Track Device was not found.");
    return rack;
}

std::optional<TrackDeviceStatusSpec> TimelineEngine::deviceStatus(const juce::String& trackId,
                                                                  const juce::String& deviceId,
                                                                  juce::String& error) const {
    return graphRegistry.access(
        [&](const ControlGraphRegistry::State& graphs) -> std::optional<TrackDeviceStatusSpec> {
            const auto* rack = findCommittedRack(graphs, trackId, deviceId, "status", error);
            if (rack == nullptr) return std::nullopt;
            const auto rackStatus = rack->status();
            const auto parameterCount = static_cast<std::uint32_t>(rack->parameterCount());
            return TrackDeviceStatusSpec{
                rackStatus.name,
                rackStatus.bypassed,
                parameterCount,
                {parameterCount > 0, true, rack->hasPrograms(), rack->hasEditor()},
            };
        });
}

std::optional<TrackDeviceParametersSpec> TimelineEngine::deviceParameterStatus(
    const juce::String& trackId, const juce::String& deviceId, juce::String& error) const {
    return graphRegistry.access(
        [&](const ControlGraphRegistry::State& graphs) -> std::optional<TrackDeviceParametersSpec> {
            const auto* rack = findCommittedRack(graphs, trackId, deviceId, "parameters", error);
            if (rack == nullptr) return std::nullopt;
            TrackDeviceParametersSpec result;
            for (const auto& parameter : rack->parameters())
                result.parameters.push_back({static_cast<std::uint32_t>(parameter.index),
                                             parameter.name, parameter.value,
                                             parameter.defaultValue, parameter.automatable});
            return result;
        });
}

std::optional<TrackDeviceProgramsSpec> TimelineEngine::deviceProgramStatus(
    const juce::String& trackId, const juce::String& deviceId, juce::String& error) const {
    return graphRegistry.access(
        [&](const ControlGraphRegistry::State& graphs) -> std::optional<TrackDeviceProgramsSpec> {
            const auto* rack = findCommittedRack(graphs, trackId, deviceId, "programs", error);
            if (rack == nullptr) return std::nullopt;
            const auto programs = rack->programStatus();
            if (programs.error.has_value()) {
                error = *programs.error;
                return std::nullopt;
            }
            TrackDeviceProgramsSpec result;
            if (programs.currentIndex >= 0)
                result.currentIndex = static_cast<std::uint32_t>(programs.currentIndex);
            for (std::size_t index = 0; index < programs.names.size(); ++index)
                result.programs.push_back(
                    {static_cast<std::uint32_t>(index), programs.names[index]});
            return result;
        });
}

std::optional<PluginStateSpec> TimelineEngine::devicePersistedState(const juce::String& trackId,
                                                                    const juce::String& deviceId,
                                                                    juce::String& error) const {
    return graphRegistry.access(
        [&](const ControlGraphRegistry::State& graphs) -> std::optional<PluginStateSpec> {
            const auto* rack = findCommittedRack(graphs, trackId, deviceId, "state", error);
            if (rack == nullptr) return std::nullopt;
            return rack->persistedState(error);
        });
}

bool TimelineEngine::mirrorEditorDeviceState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const PluginStateSpec& persistedState,
                                             juce::String& error) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        auto* track = graphs.latestCommitted != nullptr ? graphs.latestCommitted->findTrack(trackId)
                                                        : nullptr;
        if (track == nullptr) {
            error = graphs.latestCommitted == nullptr ? "Timeline is not loaded."
                                                      : "Track was not found.";
            return false;
        }
        auto* rack = findRack(*track, deviceId);
        if (rack == nullptr) {
            error = isInstrumentDevice(*track, deviceId)
                        ? "Built-in instruments do not provide a VST3 editor state."
                        : "Track Device was not found.";
            return false;
        }
        return rack->applyPersistedState(persistedState, error);
    });
}

bool TimelineEngine::mirrorEditorDeviceParameter(const juce::String& trackId,
                                                 const juce::String& deviceId,
                                                 const int parameterIndex, const float value,
                                                 juce::String& error) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        auto* track = graphs.latestCommitted != nullptr ? graphs.latestCommitted->findTrack(trackId)
                                                        : nullptr;
        if (track == nullptr) {
            error = graphs.latestCommitted == nullptr ? "Timeline is not loaded."
                                                      : "Track was not found.";
            return false;
        }
        auto* rack = findRack(*track, deviceId);
        if (rack == nullptr) {
            error = isInstrumentDevice(*track, deviceId)
                        ? "Built-in instruments do not expose editable parameters."
                        : "Track Device was not found.";
            return false;
        }
        rack->enqueueParameterChange(parameterIndex, value);
        return true;
    });
}

bool TimelineEngine::setDeviceBypassed(const juce::String& trackId, const juce::String& deviceId,
                                       const bool bypassed, juce::String& error) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "Arrangement Graph is not loaded.";
            return false;
        }
        auto* track = graphs.latestCommitted->findTrack(trackId);
        if (track == nullptr) {
            error = "Track was not found.";
            return false;
        }
        if (isInstrumentDevice(*track, deviceId)) {
            if (track->runtime->instrument() == nullptr) {
                error = "Instrument runtime is not loaded.";
                return false;
            }
            track->runtime->instrument()->setBypassed(bypassed);
            return true;
        }
        auto* effect = track->runtime->effects().findDevice(deviceId);
        if (effect == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        effect->setBypassed(bypassed);
        return true;
    });
}

bool TimelineEngine::setDeviceParameter(const juce::String& trackId, const juce::String& deviceId,
                                        const int parameterIndex, const float value,
                                        juce::String& error) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "Arrangement Graph is not loaded.";
            return false;
        }
        auto* track = graphs.latestCommitted->findTrack(trackId);
        if (track == nullptr) {
            error = "Track was not found.";
            return false;
        }
        auto* rack = findRack(*track, deviceId);
        if (rack == nullptr) {
            error = isInstrumentDevice(*track, deviceId)
                        ? "Built-in instruments do not expose editable parameters."
                        : "Track Device was not found.";
            return false;
        }
        if (parameterIndex < 0 ||
            static_cast<std::size_t>(parameterIndex) >= rack->parameterCount()) {
            error = "Track Device parameter index is invalid.";
            return false;
        }
        return rack->setParameter(parameterIndex, value, error);
    });
}

bool TimelineEngine::setDevicePersistedState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const PluginStateSpec& persistedState,
                                             juce::String& error) {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "Arrangement Graph is not loaded.";
            return false;
        }
        auto* track = graphs.latestCommitted->findTrack(trackId);
        if (track == nullptr) {
            error = "Track was not found.";
            return false;
        }
        auto* rack = findRack(*track, deviceId);
        if (rack == nullptr) {
            error = isInstrumentDevice(*track, deviceId)
                        ? "Built-in instruments do not expose plugin state."
                        : "Track Device was not found.";
            return false;
        }
        return rack->applyPersistedState(persistedState, error);
    });
}

bool TimelineEngine::setDeviceProgram(const juce::String& trackId, const juce::String& deviceId,
                                      const int programIndex, juce::String& error) {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "Arrangement Graph is not loaded.";
            return false;
        }
        auto* track = graphs.latestCommitted->findTrack(trackId);
        if (track == nullptr) {
            error = "Track was not found.";
            return false;
        }
        auto* rack = findRack(*track, deviceId);
        if (rack == nullptr) {
            error = isInstrumentDevice(*track, deviceId)
                        ? "Built-in instruments do not expose plugin programs."
                        : "Track Device was not found.";
            return false;
        }
        const auto programs = rack->programStatus();
        if (programs.error.has_value()) {
            error = *programs.error;
            return false;
        }
        if (programIndex < 0 || static_cast<std::size_t>(programIndex) >= programs.names.size()) {
            error = "Plugin program index is out of range.";
            return false;
        }
        return rack->setProgram(programIndex, error);
    });
}

}  // namespace riffra
