#include <algorithm>
#include <cmath>
#include <limits>
#include <map>
#include <new>
#include <regex>
#include <vector>

#include "FaultInjection.h"
#include "PluginRack.h"

namespace riffra {

void PluginRack::addProcessorListener(juce::AudioProcessorListener& listener) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->addListener(&listener);
}

void PluginRack::removeProcessorListener(juce::AudioProcessorListener& listener) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->removeListener(&listener);
}

void PluginRack::queueParameterValue(const int index, const float value) noexcept {
    auto* queue = parameterQueue.get();
    if (queue == nullptr || index < 0 || static_cast<std::size_t>(index) >= queue->capacity) {
        return;
    }
    const auto offset = static_cast<std::size_t>(index);
    staticStateDirty.store(true, std::memory_order_release);
    const auto normalized = juce::jlimit(0.0f, 1.0f, value);
    queue->values[offset].store(normalized, std::memory_order_release);
    queue->dirty[offset].store(true, std::memory_order_release);
}

void PluginRack::enqueueParameterChange(const int index, const float value) noexcept {
    queueParameterValue(index, value);
}

std::size_t PluginRack::parameterCount() const noexcept {
    return cachedParameterCount.load(std::memory_order_acquire);
}

bool PluginRack::canQueueParameterValue(const int index, juce::String& error) const noexcept {
    const auto* queue = parameterQueue.get();
    if (queue == nullptr || index < 0 || static_cast<std::size_t>(index) >= queue->capacity) {
        error = "Plugin parameter change queue cannot handle this index.";
        return false;
    }
    return true;
}

bool PluginRack::hasQueuedParameterChanges() const noexcept {
    auto* queue = parameterQueue.get();
    if (queue == nullptr) return false;
    for (std::size_t index = 0; index < queue->capacity; ++index)
        if (queue->dirty[index].load(std::memory_order_acquire)) return true;
    return false;
}

bool PluginRack::hasPrograms() const noexcept {
    return cachedProgramCount.load(std::memory_order_acquire) > 0;
}

bool PluginRack::allocateParameterQueue(const std::size_t count, juce::String& error) noexcept {
    std::unique_ptr<ParameterQueue> candidate(new (std::nothrow) ParameterQueue(count));
    if (candidate == nullptr) {
        error = "The plugin parameter change queue could not be allocated.";
        return false;
    }
    if (count > 0) {
        candidate->automationRestore = std::unique_ptr<ParameterQueue::Restore[]>(
            new (std::nothrow) ParameterQueue::Restore[count]);
        if (candidate->automationRestore == nullptr) {
            error = "plugin automation restore storage could not be allocated";
            return false;
        }
        candidate->values =
            std::unique_ptr<std::atomic<float>[]>(new (std::nothrow) std::atomic<float>[count]);
        candidate->dirty =
            std::unique_ptr<std::atomic<bool>[]>(new (std::nothrow) std::atomic<bool>[count]);
        candidate->automationPreview = std::unique_ptr<AutomationPreviewValue[]>(
            new (std::nothrow) AutomationPreviewValue[count]);
        if (candidate->values == nullptr || candidate->dirty == nullptr ||
            candidate->automationPreview == nullptr) {
            error = "The plugin parameter change queue could not be allocated.";
            return false;
        }
        for (std::size_t index = 0; index < count; ++index) {
            candidate->values[index].store(0.0f, std::memory_order_relaxed);
            candidate->dirty[index].store(false, std::memory_order_relaxed);
        }
    }
    parameterQueue = std::move(candidate);
    return true;
}

juce::AudioProcessorEditor* PluginRack::createEditor(juce::String& error) {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return nullptr;
    }
    try {
        if (!plugin->hasEditor()) {
            error = "The loaded VST3 does not provide an editor.";
            return nullptr;
        }
        return plugin->createEditorAndMakeActive();
    } catch (const std::exception& exception) {
        error = "VST3 editor creation raised an exception: " + juce::String(exception.what());
    } catch (...) {
        error = "VST3 editor creation failed with an unknown exception.";
    }
    return nullptr;
}

juce::String PluginRack::currentPluginName() const {
    const juce::ScopedLock lock(statusLock);
    return pluginName;
}

bool PluginRack::setParameter(const int index, const float value, juce::String& error) noexcept {
    if (isAutomationActive()) {
        error = "device state editing is unavailable during automation read";
        return false;
    }
    if (!loaded.load(std::memory_order_acquire)) {
        error = "No VST3 plugin is loaded.";
        return false;
    }
    if (index < 0 || static_cast<std::size_t>(index) >= parameterCount()) {
        error = "Plugin parameter index is out of range.";
        return false;
    }
    if (!canQueueParameterValue(index, error)) return false;
    enqueueParameterChange(index, value);
    return true;
}

bool PluginRack::setProgram(const int index, juce::String& error) {
    if (isAutomationActive()) {
        error = "device state editing is unavailable during automation read";
        return false;
    }
    staticStateDirty.store(true, std::memory_order_release);
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return false;
    }
    try {
        const auto programCount = plugin->getNumPrograms();
        if (index < 0 || index >= programCount) {
            error = "Plugin program index is out of range.";
            return false;
        }
        plugin->setCurrentProgram(index);
        updateParameterCount(*plugin);
        return true;
    } catch (const std::exception& exception) {
        error = "VST3 program change raised an exception: " + juce::String(exception.what());
    } catch (...) {
        error = "VST3 program change failed with an unknown exception.";
    }
    return false;
}

bool PluginRack::applyStateData(const juce::String& base64, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return false;
    }
    juce::MemoryBlock state;
    juce::MemoryOutputStream output(state, true);
    if (!base64.isEmpty() && !juce::Base64::convertFromBase64(output, base64)) {
        error = "VST3 state data is not valid Base64.";
        return false;
    }
    output.flush();
    plugin->setStateInformation(state.getData(), static_cast<int>(state.getSize()));
    updateParameterCount(*plugin);
    return true;
}

bool PluginRack::applyPersistedState(const PluginStateSpec& state, juce::String& error) noexcept {
    if (isAutomationActive()) {
        error = "device state editing is unavailable during automation read";
        return false;
    }
    PluginStateSpec staticProjection;
    try {
        staticProjection = state;
        if (!staticProjection.stateData || staticProjection.stateData->isEmpty()) {
            const juce::SpinLock::ScopedLockType lock(pluginLock);
            if (plugin == nullptr) {
                error = "No VST3 plugin is loaded.";
                return false;
            }
            const auto& parameters = plugin->getParameters();
            const auto existingCount = staticProjection.parameterValues.size();
            staticProjection.parameterValues.reserve(
                std::max(existingCount, static_cast<std::size_t>(parameters.size())));
            for (auto index = existingCount; index < static_cast<std::size_t>(parameters.size());
                 ++index) {
                const auto* parameter = parameters[static_cast<int>(index)];
                staticProjection.parameterValues.push_back(
                    parameter != nullptr ? parameter->getDefaultValue() : 0.0f);
            }
        }
    } catch (...) {
        error = "plugin static state projection could not be prepared";
        return false;
    }
    staticStateDirty.store(true, std::memory_order_release);
    try {
        FaultInjection::before(FaultStage::stateApply);
    } catch (const std::exception& exception) {
        error =
            "Fault injection interrupted VST3 state application: " + juce::String(exception.what());
        return false;
    } catch (...) {
        error = "Fault injection interrupted VST3 state application.";
        return false;
    }
    if (state.stateData.has_value() && state.stateData->isNotEmpty()) {
        if (!applyStateData(*state.stateData, error)) return false;
    }
    {
        const juce::SpinLock::ScopedLockType lock(pluginLock);
        if (plugin == nullptr) {
            error = "No VST3 plugin is loaded.";
            return false;
        }
        const auto& available = plugin->getParameters();
        const auto count =
            std::min(state.parameterValues.size(), static_cast<std::size_t>(available.size()));
        for (std::size_t offset = 0; offset < count; ++offset) {
            const auto index = static_cast<int>(offset);
            const auto* parameter = available[index];
            if (parameter == nullptr) continue;
            const auto target = state.parameterValues[offset];
            const auto current =
                parameterQueue != nullptr && offset < parameterQueue->capacity &&
                        parameterQueue->dirty[offset].load(std::memory_order_acquire)
                    ? parameterQueue->values[offset].load(std::memory_order_acquire)
                    : parameter->getValue();
            // VST instruments often expose thousands of parameters, most of
            // which are already at their saved/default value immediately
            // after construction. Avoid calling third-party automation hooks
            // for those entries; applying them all can turn a normal restore
            // into a multi-second operation even though no state changes.
            if (std::isfinite(target) && std::isfinite(current) &&
                std::abs(target - current) <= 0.000001f)
                continue;
            if (!setParameter(index, target, error)) return false;
        }
    }
    setBypassed(state.bypassed);
    try {
        const juce::SpinLock::ScopedLockType lock(pluginLock);
        staticState = std::move(staticProjection);
        staticStateDirty.store(false, std::memory_order_release);
    } catch (...) {
        error = "plugin static state projection could not be retained";
        return false;
    }
    return true;
}

std::optional<PluginStateSpec> PluginRack::persistedState(juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return std::nullopt;
    }
    if (staticState && (!staticStateDirty.load(std::memory_order_acquire) || isAutomationActive()))
        return staticState;
    PluginStateSpec result;
    try {
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        if (!state.isEmpty())
            result.stateData = juce::Base64::toBase64(state.getData(), state.getSize());
    } catch (const std::exception& exception) {
        error = "VST3 state capture raised an exception: " + juce::String(exception.what());
        return std::nullopt;
    } catch (...) {
        error = "VST3 state capture failed with an unknown exception.";
        return std::nullopt;
    }
    const auto& parameters = plugin->getParameters();
    result.parameterValues.reserve(static_cast<std::size_t>(parameters.size()));
    for (auto* parameter : parameters)
        result.parameterValues.push_back(parameter != nullptr ? parameter->getValue() : 0.0f);
    result.bypassed = bypassed.load(std::memory_order_acquire);
    if (staticState) {
        staticState = result;
        staticStateDirty.store(false, std::memory_order_release);
    }
    return result;
}

void PluginRack::restoreAutomation() noexcept {
    auto* queue = parameterQueue.get();
    if (queue != nullptr) {
        for (std::size_t index = 0; index < queue->capacity; ++index) {
            auto& restore = queue->automationRestore[index];
            if (restore.parameter == nullptr) continue;
            restore.parameter->setValue(restore.value);
            restore.parameter = nullptr;
        }
    }
    activeBindings = nullptr;
    activeBindingCount = 0;
    automationActive.store(false, std::memory_order_release);
}

void PluginRack::applyAutomation(std::span<PluginParameterAutomation> bindings,
                                 const std::int64_t timelineSample, const bool playing) noexcept {
    if (!playing || activeBindings != bindings.data() || activeBindingCount != bindings.size()) {
        restoreAutomation();
        activeBindings = bindings.data();
        activeBindingCount = bindings.size();
    }
    if (!playing) return;
    for (auto& binding : bindings) {
        auto& restore = parameterQueue->automationRestore[binding.parameterIndex];
        if (restore.parameter == nullptr) restore = {binding.parameter, binding.staticValue};
        auto cursor = binding.lane.cursorAt(timelineSample - binding.inputLatency);
        const auto preview = parameterQueue->automationPreview[binding.parameterIndex].current();
        const auto value = preview.value_or(
            cursor.valueAt(timelineSample - binding.inputLatency, binding.staticValue));
        if (binding.parameter->getValue() != value) binding.parameter->setValue(value);
        automationActive.store(true, std::memory_order_release);
    }
}

void PluginRack::applyQueuedParameterChanges() noexcept {
    auto* processor = plugin.get();
    auto* queue = parameterQueue.get();
    if (processor == nullptr || queue == nullptr) return;
    const auto& parameters = processor->getParameters();
    const auto count = std::min(parameters.size(), static_cast<int>(queue->capacity));
    for (int index = 0; index < count; ++index) {
        if (!queue->dirty[static_cast<std::size_t>(index)].exchange(false,
                                                                    std::memory_order_acq_rel))
            continue;
        if (auto* parameter = parameters[index])
            parameter->setValueNotifyingHost(
                queue->values[static_cast<std::size_t>(index)].load(std::memory_order_acquire));
    }
}

namespace {

juce::String normalizedDisplayText(const juce::String& text) {
    return text.toLowerCase().removeCharacters(" \t\r\n");
}

struct NumericDisplay {
    double value;
    juce::String unit;
};

std::optional<NumericDisplay> parseNumericDisplay(const juce::String& text,
                                                  const juce::String& fallbackUnit = {}) {
    static const std::regex number(R"(^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+))\s*([^0-9]*)$)");
    std::smatch match;
    const auto input = text.toStdString();
    if (!std::regex_match(input, match, number)) return std::nullopt;
    try {
        const auto value = std::stod(match[1].str());
        if (!std::isfinite(value)) return std::nullopt;
        auto unit = normalizedDisplayText(juce::String(match[2].str()));
        if (unit.isEmpty()) unit = normalizedDisplayText(fallbackUnit);
        return NumericDisplay{value, unit};
    } catch (...) {
        return std::nullopt;
    }
}

std::optional<float> nearestNumericValue(const juce::AudioProcessorParameter& parameter,
                                         const NumericDisplay& requested, juce::String& error) {
    const auto fail = [&error](const juce::String& reason) -> std::optional<float> {
        error = "Plugin display approximation failed: " + reason;
        return std::nullopt;
    };
    const auto read = [&](const float position) -> std::optional<double> {
        try {
            const auto first = parameter.getText(position, 96);
            const auto second = parameter.getText(position, 96);
            if (normalizedDisplayText(first) != normalizedDisplayText(second)) return std::nullopt;
            // A gain control may display negative infinity at silence without a unit.
            const auto text = normalizedDisplayText(first);
            const auto label = normalizedDisplayText(parameter.getLabel());
            if (requested.unit == "db" && (label.isEmpty() || label == "db") &&
                (text == "-inf" || text == "-infinity" || text == "-infdb" ||
                 text == "-infinitydb"))
                return -std::numeric_limits<double>::infinity();
            const auto parsed = parseNumericDisplay(first, parameter.getLabel());
            if (!parsed || parsed->unit != requested.unit) return std::nullopt;
            return parsed->value;
        } catch (...) {
            return std::nullopt;
        }
    };
    const auto discrete = parameter.isDiscrete();
    const auto steps = discrete ? parameter.getNumSteps() : 257;
    if (steps < 2 || steps > 4096) return fail("unsupported parameter steps.");
    std::vector<double> samples;
    samples.reserve(static_cast<std::size_t>(steps));
    auto bestPosition = 0.0f;
    auto bestError = std::numeric_limits<double>::infinity();
    const auto consider = [&](const float position, const double value) {
        if (const auto distance = std::abs(value - requested.value); distance < bestError) {
            bestError = distance;
            bestPosition = position;
        }
    };
    for (int i = 0; i < steps; ++i) {
        const auto position = static_cast<float>(i) / static_cast<float>(steps - 1);
        const auto value = read(position);
        if (!value) return fail("display is non-numeric, unstable, or has a different unit.");
        samples.push_back(*value);
        consider(position, *value);
    }
    const auto increasing = samples.back() >= samples.front();
    for (std::size_t i = 1; i < samples.size(); ++i) {
        if ((increasing && samples[i] < samples[i - 1]) ||
            (!increasing && samples[i] > samples[i - 1]))
            return fail("display is not monotonic.");
    }
    if (requested.value < std::min(samples.front(), samples.back()) ||
        requested.value > std::max(samples.front(), samples.back()))
        return fail("requested value is outside the displayed range.");
    if (!discrete) {
        auto low = 0.0f, high = 1.0f;
        auto lowValue = samples.front(), highValue = samples.back();
        for (int i = 0; i < 24; ++i) {
            const auto mid = (low + high) * 0.5f;
            if (mid == low || mid == high) break;
            const auto value = read(mid);
            if (!value) return fail("display is non-numeric, unstable, or has a different unit.");
            if ((increasing && (*value < lowValue || *value > highValue)) ||
                (!increasing && (*value > lowValue || *value < highValue)))
                return fail("display is not monotonic.");
            consider(mid, *value);
            if ((increasing && *value < requested.value) ||
                (!increasing && *value > requested.value)) {
                low = mid;
                lowValue = *value;
            } else {
                high = mid;
                highValue = *value;
            }
        }
    }
    const auto verified = read(bestPosition);
    if (!verified || std::abs(*verified - requested.value) != bestError)
        return fail("display changed during conversion.");
    return bestPosition;
}

}  // namespace

bool PluginRack::equivalentDisplayText(const juce::String& input, const juce::String& output,
                                       const juce::String& label) {
    if (normalizedDisplayText(input) == normalizedDisplayText(output)) return true;
    static const std::regex number(R"(^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+))\s*([^0-9]*)$)");
    std::smatch requested, rendered;
    const auto inputText = input.toStdString(), outputText = output.toStdString();
    if (!std::regex_match(inputText, requested, number) ||
        !std::regex_match(outputText, rendered, number))
        return false;
    const auto unit = normalizedDisplayText(juce::String(requested[2].str()));
    auto renderedUnit = normalizedDisplayText(juce::String(rendered[2].str()));
    if (renderedUnit.isEmpty()) renderedUnit = normalizedDisplayText(label);
    if (unit != renderedUnit) return false;
    const auto displayNumber = rendered[1].str();
    const auto decimal = displayNumber.find('.');
    const auto precision = decimal == std::string::npos ? 0 : displayNumber.size() - decimal - 1;
    const auto tolerance = 0.5 * std::pow(10.0, -static_cast<double>(precision));
    try {
        const auto a = std::stod(requested[1].str()), b = std::stod(displayNumber);
        return std::isfinite(a) && std::isfinite(b) && std::abs(a - b) <= tolerance;
    } catch (...) {
        return false;
    }
}

std::optional<float> PluginRack::parameterValueForText(const int index, const juce::String& text,
                                                       juce::String& error,
                                                       const bool nearest) const {
    const auto& parameters = plugin->getParameters();
    if (index < 0 || index >= parameters.size() || parameters[index] == nullptr) {
        error = "Plugin parameter index is out of range.";
        return std::nullopt;
    }
    const auto* parameter = parameters[index];
    const auto requested = nearest ? parseNumericDisplay(text) : std::nullopt;
    if (nearest && !requested) {
        error = "Nearest requires numeric display text.";
        return std::nullopt;
    }
    try {
        const auto value = parameter->getValueForText(text);
        if (text.trim().isNotEmpty() && std::isfinite(value) && value >= 0.0f && value <= 1.0f &&
            equivalentDisplayText(text, parameter->getText(value, 96), parameter->getLabel())) {
            return value;
        }
        if (nearest) return nearestNumericValue(*parameter, *requested, error);
    } catch (...) {
        error = "Plugin display conversion failed.";
        return std::nullopt;
    }
    error = "Plugin cannot convert this display text unambiguously.";
    return std::nullopt;
}

std::optional<juce::String> PluginRack::parameterDisplayValue(const int index, const float value,
                                                              juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr || index < 0 || static_cast<std::size_t>(index) >= parameterCount() ||
        !std::isfinite(value) || value < 0.0f || value > 1.0f) {
        error = "Plugin parameter display request is out of range.";
        return std::nullopt;
    }
    const auto& parameters = plugin->getParameters();
    if (index >= parameters.size() || parameters[index] == nullptr) {
        error = "Plugin parameter index is out of range.";
        return std::nullopt;
    }
    try {
        return parameters[index]->getText(value, 96);
    } catch (...) {
        error = "Plugin parameter display conversion failed.";
        return std::nullopt;
    }
}

std::optional<PluginParameterReadback> PluginRack::parameterReadback(const int index,
                                                                     juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "Plugin device parameter state is unavailable.";
        return std::nullopt;
    }
    const auto& parameters = plugin->getParameters();
    if (index < 0 || static_cast<std::size_t>(index) >= parameters.size() ||
        parameters[index] == nullptr) {
        error = "Plugin parameter index is out of range.";
        return std::nullopt;
    }
    auto* parameter = parameters[index];
    try {
        const auto value = parameter->getValue();
        return PluginParameterReadback{
            value, parameter->getText(value, 96), parameter->getLabel(), parameter->isDiscrete(),
            parameter->isDiscrete()
                ? static_cast<std::uint32_t>(std::max(0, parameter->getNumSteps()))
                : 0u};
    } catch (...) {
        error = "Plugin parameter readback failed.";
        return std::nullopt;
    }
}

void PluginRack::updateParameterCount(juce::AudioProcessor& processor) {
    cachedParameterCount.store(static_cast<std::size_t>(processor.getParameters().size()),
                               std::memory_order_release);
}

void PluginRack::refreshPreparedLatencySamples() noexcept {
    const auto* processor = plugin.get();
    if (processor == nullptr) return;
    preparedLatencySamples.store(std::max(0, processor->getLatencySamples()),
                                 std::memory_order_release);
}

PluginRackStatus PluginRack::status() const {
    const juce::ScopedLock lock(statusLock);
    return {
        loaded.load(std::memory_order_acquire),
        pluginPath,
        pluginName,
        bypassed.load(std::memory_order_acquire),
        preparedSampleRate.load(std::memory_order_acquire),
        preparedBlockSize.load(std::memory_order_acquire),
        pluginInputChannels.load(std::memory_order_acquire),
        pluginOutputChannels.load(std::memory_order_acquire),
        bypassedBlocks.load(std::memory_order_acquire),
        processedBlocks.load(std::memory_order_acquire),
        transitionBlocks.load(std::memory_order_acquire),
        pendingMidi.droppedEvents(),
        loadCount.load(std::memory_order_acquire),
        destroyCount.load(std::memory_order_acquire),
        {pluginSidechainAvailable.load(std::memory_order_acquire),
         pluginSidechainMono.load(std::memory_order_acquire),
         pluginSidechainStereo.load(std::memory_order_acquire),
         pluginAuxInputChannels.load(std::memory_order_acquire), sidechainUnavailableReason},
    };
}

std::vector<PluginParameterInfo> PluginRack::parameters() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    std::vector<PluginParameterInfo> result;
    if (plugin == nullptr) return result;
    const auto& parameters = plugin->getParameters();
    result.reserve(static_cast<std::size_t>(parameters.size()));
    std::map<juce::String, int> idCounts;
    for (auto* parameter : parameters) {
        if (const auto* hosted =
                dynamic_cast<const juce::HostedAudioProcessorParameter*>(parameter)) {
            const auto id = hosted->getParameterID();
            if (id.isNotEmpty()) ++idCounts[id];
        }
    }
    for (int index = 0; index < parameters.size(); ++index) {
        auto* parameter = parameters[index];
        if (parameter == nullptr) continue;
        const auto value = parameter->getValue();
        const auto discrete = parameter->isDiscrete();
        const auto steps =
            discrete ? static_cast<std::uint32_t>(std::max(0, parameter->getNumSteps())) : 0u;
        std::optional<juce::String> parameterId;
        if (const auto* hosted =
                dynamic_cast<const juce::HostedAudioProcessorParameter*>(parameter)) {
            const auto id = hosted->getParameterID();
            if (id.isNotEmpty() && idCounts[id] == 1) parameterId = id;
        }
        PluginParameterInfo info{index,
                                 parameterId,
                                 parameter->getName(96),
                                 value,
                                 parameter->getDefaultValue(),
                                 parameter->isAutomatable(),
                                 parameter->getText(value, 96),
                                 parameter->getLabel(),
                                 discrete,
                                 steps,
                                 {}};
        if (steps >= 2 && steps <= 256) {
            info.choices.reserve(steps);
            for (std::uint32_t choice = 0; choice < steps; ++choice) {
                const auto normalized = static_cast<float>(choice) / static_cast<float>(steps - 1);
                info.choices.push_back({normalized, parameter->getText(normalized, 96)});
            }
        }
        result.push_back(std::move(info));
    }
    return result;
}

std::optional<PluginParameterAutomation> PluginRack::bindAutomation(
    const juce::String& parameterId, AutomationRuntime lane, const PluginStateSpec& state,
    const bool stepped, AutomationBindingFailure& failure, juce::String& error) {
    failure = AutomationBindingFailure::invalidParameter;
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "automation device is unavailable";
        return std::nullopt;
    }

    const auto& parameters = plugin->getParameters();
    juce::AudioProcessorParameter* resolved = nullptr;
    std::size_t index = 0;
    for (int parameterIndex = 0; parameterIndex < parameters.size(); ++parameterIndex) {
        auto* parameter = parameters[parameterIndex];
        const auto* hosted = dynamic_cast<const juce::HostedAudioProcessorParameter*>(parameter);
        if (hosted == nullptr || hosted->getParameterID() != parameterId) continue;
        if (resolved != nullptr) {
            failure = AutomationBindingFailure::laneUnavailable;
            error = "automation parameter ID is ambiguous: " + parameterId;
            return std::nullopt;
        }
        resolved = parameter;
        index = static_cast<std::size_t>(parameterIndex);
    }
    if (resolved == nullptr) {
        failure = AutomationBindingFailure::laneUnavailable;
        error = "automation parameter ID is not present: " + parameterId;
        return std::nullopt;
    }
    if (!resolved->isAutomatable() || (resolved->isDiscrete() && !stepped)) {
        failure = AutomationBindingFailure::laneUnavailable;
        error = "automation parameter is not automatable with this interpolation: " + parameterId;
        return std::nullopt;
    }
    auto staticValue = resolved->getDefaultValue();
    if (index < state.parameterValues.size())
        staticValue = state.parameterValues[index];
    else if (state.stateData.has_value() && state.stateData->isNotEmpty()) {
        error = "automation static parameter state is unavailable: " + parameterId;
        return std::nullopt;
    }
    if (!std::isfinite(staticValue) || staticValue < 0.0f || staticValue > 1.0f) {
        error = "automation static parameter state is invalid: " + parameterId;
        return std::nullopt;
    }
    if (parameterQueue == nullptr || index >= parameterQueue->capacity) {
        failure = AutomationBindingFailure::unavailableStorage;
        error = "automation parameter storage requires device reprepare";
        return std::nullopt;
    }
    return PluginParameterAutomation{this, resolved, std::move(lane), staticValue, index};
}

bool PluginRack::setAutomationPreview(const juce::String& parameterId,
                                      const std::uint64_t previewToken,
                                      const std::optional<float> value, juce::String& error) {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr || parameterQueue == nullptr) {
        error = "automation device is unavailable";
        return false;
    }
    const auto& parameters = plugin->getParameters();
    std::optional<std::size_t> resolvedIndex;
    for (int index = 0; index < parameters.size(); ++index) {
        const auto* hosted =
            dynamic_cast<const juce::HostedAudioProcessorParameter*>(parameters[index]);
        if (hosted == nullptr || hosted->getParameterID() != parameterId) continue;
        if (resolvedIndex.has_value()) {
            error = "automation parameter ID is ambiguous: " + parameterId;
            return false;
        }
        resolvedIndex = static_cast<std::size_t>(index);
    }
    if (!resolvedIndex || *resolvedIndex >= parameterQueue->capacity) {
        error = "automation parameter ID is not present: " + parameterId;
        return false;
    }
    auto& preview = parameterQueue->automationPreview[*resolvedIndex];
    if (value.has_value()) {
        if (!std::isfinite(*value) || *value < 0.0f || *value > 1.0f) {
            error = "automation preview value must be between 0 and 1";
            return false;
        }
        preview.set(previewToken, *value);
    } else {
        preview.clear(previewToken);
    }
    return true;
}

PluginProgramStatus PluginRack::programStatus() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    PluginProgramStatus result;
    if (plugin == nullptr) return result;
    try {
        const auto programCount = plugin->getNumPrograms();
        result.names.reserve(static_cast<std::size_t>(std::max(0, programCount)));
        for (int index = 0; index < programCount; ++index)
            result.names.push_back(plugin->getProgramName(index));
        const auto currentIndex = plugin->getCurrentProgram();
        result.currentIndex = currentIndex >= 0 && currentIndex < programCount ? currentIndex : -1;
    } catch (const std::exception& exception) {
        result = {};
        result.error = "VST3 program enumeration failed: " + juce::String(exception.what());
    } catch (...) {
        result = {};
        result.error = "VST3 program enumeration failed.";
    }
    return result;
}

bool PluginRack::hasEditor() const noexcept {
    return cachedHasEditor.load(std::memory_order_acquire);
}

}  // namespace riffra
