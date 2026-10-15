#include <algorithm>
#include <thread>
#include <utility>

#include "TimelineEngine.h"

namespace riffra {

namespace {

using Track = PreparedTimeline::Track;

class HostEditScope final {
public:
    explicit HostEditScope(std::atomic<bool>& flag) : editing(flag) {
        editing.store(true, std::memory_order_release);
    }
    ~HostEditScope() { editing.store(false, std::memory_order_release); }

private:
    std::atomic<bool>& editing;
};

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

PluginRack* findRack(PreparedTimeline& graph, const EffectOwnerSpec& owner,
                     const juce::String& deviceId) noexcept {
    if (owner.kind == EffectOwnerKindSpec::master)
        return graph.mixer.masterEffects->findDevice(deviceId);
    if (owner.kind == EffectOwnerKindSpec::bus) {
        const auto* bus = graph.mixer.findBus(owner.id);
        return bus != nullptr ? bus->effects->findDevice(deviceId) : nullptr;
    }
    auto* track = graph.findTrack(owner.id);
    return track != nullptr ? findRack(*track, deviceId) : nullptr;
}

bool isInstrumentDevice(const PreparedTimeline& graph, const EffectOwnerSpec& owner,
                        const juce::String& deviceId) noexcept {
    const auto* track =
        owner.kind == EffectOwnerKindSpec::track ? graph.findTrack(owner.id) : nullptr;
    return track != nullptr && isInstrumentDevice(*track, deviceId);
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
            if (runtime.hasLoadedInstrument()) (void)runtime.enqueueMidi(message);
            if (state.recordingPhase == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(track.id, event.sourceIndex, message,
                                                 state.audioClockSample);
            }
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
        for (const auto& bus : graph->mixer.buses) bus->effects->reset();
        graph->mixer.masterEffects->reset();
    });
}

std::uint64_t TimelineEngine::committedRevision() const {
    return graphRegistry.access([](const ControlGraphRegistry::State& graphs) {
        return graphs.latestCommitted != nullptr ? graphs.latestCommitted->revision : 0;
    });
}

PluginRack* TimelineEngine::findDevice(const EffectOwnerSpec& deviceOwner,
                                       const juce::String& deviceId) const noexcept {
    return graphRegistry.access([&](const ControlGraphRegistry::State& graphs) -> PluginRack* {
        return graphs.latestCommitted != nullptr
                   ? findRack(*graphs.latestCommitted, deviceOwner, deviceId)
                   : nullptr;
    });
}

PluginRack* TimelineEngine::findCommittedRack(const ControlGraphRegistry::State& graphs,
                                              const EffectOwnerSpec& deviceOwner,
                                              const juce::String& deviceId, const char* noun,
                                              juce::String& error) {
    if (graphs.latestCommitted == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return nullptr;
    }
    auto* rack = findRack(*graphs.latestCommitted, deviceOwner, deviceId);
    if (rack == nullptr)
        error = isInstrumentDevice(*graphs.latestCommitted, deviceOwner, deviceId)
                    ? juce::String("Built-in instruments do not expose plugin ") + noun + "."
                    : juce::String("Channel Device was not found.");
    return rack;
}

std::optional<DeviceStatusSpec> TimelineEngine::deviceStatus(const EffectOwnerSpec& deviceOwner,
                                                             const juce::String& deviceId,
                                                             juce::String& error) const {
    return graphRegistry.access(
        [&](const ControlGraphRegistry::State& graphs) -> std::optional<DeviceStatusSpec> {
            const auto* rack = findCommittedRack(graphs, deviceOwner, deviceId, "status", error);
            if (rack == nullptr) return std::nullopt;
            const auto rackStatus = rack->status();
            const auto parameterCount = static_cast<std::uint32_t>(rack->parameterCount());
            return DeviceStatusSpec{
                rackStatus.name,
                rackStatus.bypassed,
                parameterCount,
                {parameterCount > 0, true, rack->hasPrograms(), rack->hasEditor()},
            };
        });
}

std::optional<DeviceParametersSpec> TimelineEngine::deviceParameterStatus(
    const EffectOwnerSpec& deviceOwner, const juce::String& deviceId, juce::String& error) {
    auto* rack = findDevice(deviceOwner, deviceId);
    if (rack == nullptr) {
        error = "Channel Device was not found.";
        return std::nullopt;
    }
    if (!synchronizeDeviceParameters(*rack, error)) return std::nullopt;
    return juce::MessageManager::callSync([&]() -> std::optional<DeviceParametersSpec> {
               DeviceParametersSpec result;
               for (const auto& parameter : rack->parameters()) {
                   DeviceParameterSpec info{static_cast<std::uint32_t>(parameter.index),
                                            parameter.parameterId,
                                            parameter.name,
                                            parameter.value,
                                            parameter.defaultValue,
                                            parameter.automatable,
                                            parameter.displayValue,
                                            parameter.label,
                                            parameter.discrete,
                                            parameter.stepCount,
                                            {}};
                   for (const auto& choice : parameter.choices)
                       info.choices.push_back({choice.value, choice.displayValue});
                   result.parameters.push_back(std::move(info));
               }
               return result;
           })
        .value_or(std::nullopt);
}

std::optional<DeviceParametersSpec> TimelineEngine::deviceParameterValueForText(
    const EffectOwnerSpec& owner, const juce::String& deviceId, const juce::String& parameterId,
    const juce::String& display, juce::String& error) {
    auto parameters = deviceParameterStatus(owner, deviceId, error);
    if (!parameters) return std::nullopt;
    auto* rack = findDevice(owner, deviceId);
    const auto found = std::find_if(
        parameters->parameters.begin(), parameters->parameters.end(), [&](const auto& parameter) {
            return parameter.parameterId && *parameter.parameterId == parameterId &&
                   parameter.automatable;
        });
    if (rack == nullptr || found == parameters->parameters.end()) {
        error = "automation parameter ID is unavailable";
        return std::nullopt;
    }
    const auto normalized =
        juce::MessageManager::callSync([&] {
            return rack->parameterValueForText(static_cast<int>(found->index), display, error);
        }).value_or(std::nullopt);
    if (!normalized) return std::nullopt;
    auto result = *found;
    result.value = *normalized;
    result.displayValue = display;
    return DeviceParametersSpec{{std::move(result)}};
}

std::optional<DeviceProgramsSpec> TimelineEngine::deviceProgramStatus(
    const EffectOwnerSpec& deviceOwner, const juce::String& deviceId, juce::String& error) const {
    return juce::MessageManager::callSync([&]() -> std::optional<DeviceProgramsSpec> {
               const auto* rack = findDevice(deviceOwner, deviceId);
               if (rack == nullptr) return std::nullopt;
               const auto programs = rack->programStatus();
               if (programs.error.has_value()) {
                   error = *programs.error;
                   return std::nullopt;
               }
               DeviceProgramsSpec result;
               if (programs.currentIndex >= 0)
                   result.currentIndex = static_cast<std::uint32_t>(programs.currentIndex);
               for (std::size_t index = 0; index < programs.names.size(); ++index)
                   result.programs.push_back(
                       {static_cast<std::uint32_t>(index), programs.names[index]});
               return result;
           })
        .value_or(std::nullopt);
}

std::optional<PluginStateSpec> TimelineEngine::devicePersistedState(
    const EffectOwnerSpec& deviceOwner, const juce::String& deviceId, juce::String& error) {
    auto* target = findDevice(deviceOwner, deviceId);
    if (target == nullptr) {
        error = "Channel Device was not found.";
        return std::nullopt;
    }
    if (!synchronizeDeviceParameters(*target, error)) return std::nullopt;
    return juce::MessageManager::callSync([&]() -> std::optional<PluginStateSpec> {
               const auto* rack = findDevice(deviceOwner, deviceId);
               if (rack == nullptr) return std::nullopt;
               return rack->persistedState(error);
           })
        .value_or(std::nullopt);
}

bool TimelineEngine::setDeviceBypassed(const EffectOwnerSpec& deviceOwner,
                                       const juce::String& deviceId, const bool bypassed,
                                       juce::String& error) noexcept {
    return graphRegistry.access([&](ControlGraphRegistry::State& graphs) {
        if (graphs.latestCommitted == nullptr) {
            error = "Arrangement Graph is not loaded.";
            return false;
        }
        auto* device = findRack(*graphs.latestCommitted, deviceOwner, deviceId);
        if (device != nullptr && device->isAutomationActive()) {
            error = "device state editing is unavailable during automation read";
            return false;
        }
        if (deviceOwner.kind == EffectOwnerKindSpec::track) {
            auto* track = graphs.latestCommitted->findTrack(deviceOwner.id);
            if (track != nullptr && isInstrumentDevice(*track, deviceId)) {
                if (track->runtime->instrument() == nullptr) {
                    error = "Instrument runtime is not loaded.";
                    return false;
                }
                track->runtime->instrument()->setBypassed(bypassed);
                return true;
            }
        }
        auto* effect = findCommittedRack(graphs, deviceOwner, deviceId, "bypass", error);
        if (effect == nullptr) return false;
        effect->setBypassed(bypassed);
        return true;
    });
}

bool TimelineEngine::synchronizeDeviceParameters(PluginRack& rack, juce::String& error,
                                                 const int parameterIndex,
                                                 const float value) noexcept {
    if (rack.isAutomationActive()) return true;
    if (parameterIndex < 0 &&
        !rack.parameterControllerSyncPending.load(std::memory_order_acquire)) {
        return juce::MessageManager::callSync([&] {
                   rack.updateParameterCount(*rack.plugin);
                   return true;
               })
            .value_or(false);
    }
    RealtimeCommand command;
    command.kind = RealtimeCommand::Kind::applyDeviceParameters;
    command.pluginRack = &rack;
    command.graphSerial = graphRegistry.access([](const ControlGraphRegistry::State& graphs) {
        return graphs.latestCommitted != nullptr ? graphs.latestCommitted->serial : 0;
    });
    command.parameterIndex = parameterIndex;
    command.parameterValue = value;
    const auto sequence = submit(command);
    if (!sequence.has_value()) {
        error = "The realtime command queue is full.";
        return false;
    }
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (rack.completedControlSequence.load(std::memory_order_acquire) < *sequence) {
        if (std::chrono::steady_clock::now() < deadline) {
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
            continue;
        }
        rack.cancelledControlSequence.store(*sequence, std::memory_order_release);
        error = "Plugin parameter application timed out.";
        return false;
    }
    const auto synchronized = juce::MessageManager::callSync([&] {
        if (!rack.synchronizeParameterController(error)) return false;
        rack.updateParameterCount(*rack.plugin);
        rack.preparedLatencySamples.store(std::max(0, rack.plugin->getLatencySamples()),
                                          std::memory_order_release);
        return true;
    });
    return synchronized.value_or(false);
}

bool TimelineEngine::setDeviceParameter(const EffectOwnerSpec& deviceOwner,
                                        const juce::String& deviceId, const int parameterIndex,
                                        const std::optional<float> value, juce::String& error,
                                        const std::optional<juce::String>& display) noexcept {
    auto* rack = findDevice(deviceOwner, deviceId);
    if (rack == nullptr) {
        error = "Channel Device was not found.";
        return false;
    }
    if (rack->isAutomationActive()) {
        error = "device state editing is unavailable during automation read";
        return false;
    }
    if (parameterIndex < 0 || static_cast<std::size_t>(parameterIndex) >= rack->parameterCount()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    if (value.has_value() == display.has_value()) {
        error = "Select exactly one of value or display.";
        return false;
    }
    if (!rack->canQueueParameterValue(parameterIndex, error)) return false;
    auto normalized = value;
    if (display.has_value()) {
        normalized = juce::MessageManager::callSync([&] {
                         return rack->parameterValueForText(parameterIndex, *display, error);
                     }).value_or(std::nullopt);
        if (!normalized.has_value()) return false;
    }
    const HostEditScope edit(rack->hostEditInProgress);
    return synchronizeDeviceParameters(*rack, error, parameterIndex, *normalized);
}

bool TimelineEngine::setDevicePersistedState(const EffectOwnerSpec& deviceOwner,
                                             const juce::String& deviceId,
                                             const PluginStateSpec& persistedState,
                                             juce::String& error) {
    auto* rack = findDevice(deviceOwner, deviceId);
    if (rack == nullptr) {
        error = "Channel Device was not found.";
        return false;
    }
    const HostEditScope edit(rack->hostEditInProgress);
    const auto applied = juce::MessageManager::callSync(
        [&] { return rack->applyPersistedState(persistedState, error); });
    return applied.value_or(false) && synchronizeDeviceParameters(*rack, error);
}

bool TimelineEngine::setDeviceProgram(const EffectOwnerSpec& deviceOwner,
                                      const juce::String& deviceId, const int programIndex,
                                      juce::String& error) {
    auto* rack = findDevice(deviceOwner, deviceId);
    if (rack == nullptr) {
        error = "Channel Device was not found.";
        return false;
    }
    const HostEditScope edit(rack->hostEditInProgress);
    const auto applied =
        juce::MessageManager::callSync([&] { return rack->setProgram(programIndex, error); });
    return applied.value_or(false) && synchronizeDeviceParameters(*rack, error);
}

}  // namespace riffra
