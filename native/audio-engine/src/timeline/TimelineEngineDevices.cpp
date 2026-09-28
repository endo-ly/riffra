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

bool TimelineEngine::enqueueLiveMidi(const juce::MidiMessage& message,
                                     const juce::String& deviceId) noexcept {
    const auto frame = realtimeFrame.read();
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.find(frame.activeGraphSerial);
        if (graph == nullptr || !graph->summary.armedInstrumentTrack) return false;
        for (auto& trackPtr : graph->tracks) {
            auto& track = *trackPtr;
            if (track.runtime->instrumentTrack && track.runtime->armed &&
                ArrangementGraph::midiRouteMatches(track.runtime->midiDeviceId,
                                                   track.runtime->midiChannel, deviceId,
                                                   message.getChannel())) {
                if (track.runtime->hasLoadedInstrument()) (void)track.runtime->enqueueMidi(message);
                if (frame.recordingPhase == RecordingPhase::recording)
                    recordingCapture->writeMidiTrack(track.id, deviceId, message,
                                                     frame.audioClockSample);
            }
        }
        return true;
    });
}

bool TimelineEngine::enqueueTargetedMidi(const juce::String& trackId,
                                         const juce::MidiMessage& message,
                                         juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI input.";
        return false;
    }
    const auto frame = realtimeFrame.read();
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.find(frame.activeGraphSerial);
        if (graph == nullptr) {
            error = "The Arrangement Graph is unavailable for targeted MIDI.";
            return false;
        }
        auto* track = graph->findTrack(trackId);
        if (track == nullptr) {
            error = "The target Track is not available in the Arrangement Graph.";
            return false;
        }
        if (!track->runtime->instrumentTrack || !track->runtime->hasLoadedInstrument()) {
            error = "The target Instrument Track has no loaded instrument.";
            return false;
        }
        if (!track->runtime->enqueueMidi(message)) {
            error = "The target Instrument Track could not queue MIDI.";
            return false;
        }
        if (track->runtime->armed && frame.recordingPhase == RecordingPhase::recording)
            recordingCapture->writeMidiTrack(track->id, "riffra:play-surface", message,
                                             frame.audioClockSample);
        return true;
    });
}

bool TimelineEngine::panicTargetedMidi(const juce::String& trackId, juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI panic.";
        return false;
    }
    const auto frame = realtimeFrame.read();
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        const auto* graph = graphs.find(frame.activeGraphSerial);
        if (graph == nullptr) {
            error = "The Arrangement Graph is unavailable for targeted MIDI panic.";
            return false;
        }
        auto* track = graph->findTrack(trackId);
        if (track == nullptr) {
            error = "The target Track is not available in the Arrangement Graph.";
            return false;
        }
        if (!track->runtime->instrumentTrack || !track->runtime->hasLoadedInstrument()) {
            error = "The target Instrument Track has no loaded instrument.";
            return false;
        }
        track->runtime->panic();
        return true;
    });
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
