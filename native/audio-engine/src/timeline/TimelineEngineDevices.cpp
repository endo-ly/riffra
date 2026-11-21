#include <algorithm>
#include <cmath>
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

// Continuous parameters allow drifting by the plugin's normalized float
// precision; discrete parameters accept the slot the plugin rounds into.
constexpr float kAppliedParameterTolerance = 1.0e-4f;

std::optional<float> expectedNormalizedValue(const PluginParameterReadback& readback,
                                             const float requested) {
    if (!readback.discrete || readback.stepCount < 2) return requested;
    const auto slots = static_cast<float>(readback.stepCount - 1);
    const auto slot = std::clamp(std::round(requested * slots), 0.0f, slots);
    return slot / slots;
}

/// Verifies that the applied parameter reached the requested value, and that a
/// requested display string renders equivalently. The plugin is read back on
/// the message thread after the application block completed, so the default
/// VST3 synchronization has already published the effective value.
bool verifyDeviceParameterApplication(PluginRack& rack, const int parameterIndex,
                                      const float requested,
                                      const std::optional<juce::String>& requestedDisplay,
                                      juce::String& error) {
    const auto readback =
        juce::MessageManager::callSync([&]() -> std::optional<PluginParameterReadback> {
            return rack.parameterReadback(parameterIndex, error);
        }).value_or(std::nullopt);
    if (!readback.has_value()) return false;
    const auto expected = expectedNormalizedValue(*readback, requested);
    if (const auto drift = std::abs(readback->value - *expected);
        drift > kAppliedParameterTolerance) {
        error = juce::String::formatted(
            "Plugin parameter %d did not accept the requested value: requested %.6f, "
            "effective %.6f.",
            parameterIndex, requested, readback->value);
        return false;
    }
    if (!requestedDisplay.has_value()) return true;
    if (!PluginRack::equivalentDisplayText(*requestedDisplay, readback->displayValue,
                                           readback->label)) {
        error = juce::String::formatted(
            "Plugin parameter %d did not accept the requested value: requested \"%s\", "
            "effective \"%s\".",
            parameterIndex, requestedDisplay->toRawUTF8(), readback->displayValue.toRawUTF8());
        return false;
    }
    return true;
}

}  // namespace

PreparedTimeline::Track* TimelineEngine::findTrackByKey(const RealtimeState& state,
                                                        const std::uint32_t trackKey) noexcept {
    if (state.graph == nullptr) return nullptr;
    for (auto& track : state.graph->tracks)
        if (track->runtime->key == trackKey) return track.get();
    return nullptr;
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

std::shared_ptr<PluginRack> TimelineEngine::deviceHandle(const EffectOwnerSpec& owner,
                                                         const juce::String& deviceId) const {
    return graphRegistry.access([&](const ControlGraphRegistry::State& graphs) {
        auto* graph = graphs.latestCommitted;
        if (graph == nullptr) return std::shared_ptr<PluginRack>{};
        std::shared_ptr<PluginChain> chain;
        if (owner.kind == EffectOwnerKindSpec::master) {
            chain = graph->mixer.masterEffects;
        } else if (owner.kind == EffectOwnerKindSpec::bus) {
            auto* bus = graph->mixer.findBus(owner.id);
            if (bus != nullptr) chain = bus->effects;
        } else if (auto* track = graph->findTrack(owner.id)) {
            if (isInstrumentDevice(*track, deviceId)) return track->runtime->instrumentRackHandle();
            chain = track->runtime->effectChainHandle();
        }
        auto* rack = chain != nullptr ? chain->findDevice(deviceId) : nullptr;
        return rack != nullptr ? std::shared_ptr<PluginRack>(chain, rack) : nullptr;
    });
}

bool TimelineEngine::preparedDeviceIsReused(const EffectOwnerSpec& owner,
                                            const juce::String& deviceId) const {
    return graphRegistry.access([&](const ControlGraphRegistry::State& graphs) {
        if (graphs.pending == nullptr || graphs.latestCommitted == nullptr) return false;
        auto* current = findRack(*graphs.latestCommitted, owner, deviceId);
        return current != nullptr && current == findRack(*graphs.pending, owner, deviceId);
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
            const auto& sidechain = rackStatus.sidechain;
            return DeviceStatusSpec{
                rackStatus.name,
                rackStatus.bypassed,
                parameterCount,
                {parameterCount > 0,
                 true,
                 rack->hasPrograms(),
                 rack->hasEditor(),
                 {sidechain.available, sidechain.mono, sidechain.stereo,
                  static_cast<std::uint32_t>(sidechain.channels), sidechain.reason}},
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

std::optional<DeviceParametersSpec> TimelineEngine::deviceParameterDisplayValue(
    const EffectOwnerSpec& owner, const juce::String& deviceId, const std::uint32_t parameterIndex,
    const float value, juce::String& error) {
    auto* rack = findDevice(owner, deviceId);
    if (rack == nullptr) {
        error = "Channel Device was not found.";
        return std::nullopt;
    }
    const auto display =
        juce::MessageManager::callSync([&] {
            return rack->parameterDisplayValue(static_cast<int>(parameterIndex), value, error);
        }).value_or(std::nullopt);
    if (!display) return std::nullopt;
    DeviceParameterSpec parameter;
    parameter.index = parameterIndex;
    parameter.value = value;
    parameter.automatable = true;
    parameter.displayValue = *display;
    return DeviceParametersSpec{{std::move(parameter)}};
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
    if (parameterIndex < 0 && !rack.hasQueuedParameterChanges()) {
        // Persisted-state and program operations already ran on the message
        // thread; without queued changes only the cached count and latency
        // need a refresh.
        return juce::MessageManager::callSync([&] {
                   rack.updateParameterCount(*rack.plugin);
                   rack.refreshPreparedLatencySamples();
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
    return juce::MessageManager::callSync([&] {
               rack.updateParameterCount(*rack.plugin);
               rack.refreshPreparedLatencySamples();
               return true;
           })
        .value_or(false);
}

bool TimelineEngine::setDeviceParameter(const EffectOwnerSpec& deviceOwner,
                                        const juce::String& deviceId, const int parameterIndex,
                                        const std::optional<float> value, juce::String& error,
                                        const std::optional<juce::String>& display,
                                        const bool nearest) noexcept {
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
    if (nearest && !display.has_value()) {
        error = "Nearest requires numeric display text.";
        return false;
    }
    if (!rack->canQueueParameterValue(parameterIndex, error)) return false;
    auto normalized = value;
    if (display.has_value()) {
        normalized =
            juce::MessageManager::callSync([&] {
                return rack->parameterValueForText(parameterIndex, *display, error, nearest);
            }).value_or(std::nullopt);
        if (!normalized.has_value()) return false;
    }
    auto expectedDisplay = display;
    if (nearest) {
        expectedDisplay =
            juce::MessageManager::callSync([&] {
                return rack->parameterDisplayValue(parameterIndex, *normalized, error);
            }).value_or(std::nullopt);
        if (!expectedDisplay.has_value()) return false;
    }
    const HostEditScope edit(rack->hostEditInProgress);
    if (!synchronizeDeviceParameters(*rack, error, parameterIndex, *normalized)) return false;
    return verifyDeviceParameterApplication(*rack, parameterIndex, *normalized, expectedDisplay,
                                            error);
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
