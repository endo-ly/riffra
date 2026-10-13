#include <algorithm>
#include <cmath>
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
    const auto normalized = juce::jlimit(0.0f, 1.0f, value);
    queue->values[offset].store(normalized, std::memory_order_release);
    queue->dirty[offset].store(true, std::memory_order_release);
    parameterControllerSyncPending.store(true, std::memory_order_release);
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
        candidate->values =
            std::unique_ptr<std::atomic<float>[]>(new (std::nothrow) std::atomic<float>[count]);
        candidate->dirty =
            std::unique_ptr<std::atomic<bool>[]>(new (std::nothrow) std::atomic<bool>[count]);
        if (candidate->values == nullptr || candidate->dirty == nullptr) {
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
        parameterControllerSyncPending.store(true, std::memory_order_release);
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
    plugin->setStateInformation(state.getData(), static_cast<int>(state.getSize()));
    parameterControllerSyncPending.store(true, std::memory_order_release);
    updateParameterCount(*plugin);
    return true;
}

bool PluginRack::applyPersistedState(const PluginStateSpec& state, juce::String& error) noexcept {
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
        const auto available = parameters();
        const auto count = std::min(state.parameterValues.size(), available.size());
        for (std::size_t offset = 0; offset < count; ++offset) {
            const auto index = static_cast<int>(offset);
            const auto target = state.parameterValues[offset];
            const auto availableIndex = available[offset].index;
            const auto current =
                parameterQueue != nullptr && offset < parameterQueue->capacity &&
                        parameterQueue->dirty[offset].load(std::memory_order_acquire)
                    ? parameterQueue->values[offset].load(std::memory_order_acquire)
                    : available[offset].value;
            // VST instruments often expose thousands of parameters, most of
            // which are already at their saved/default value immediately
            // after construction. Avoid calling third-party automation hooks
            // for those entries; applying them all can turn a normal restore
            // into a multi-second operation even though no state changes.
            if (availableIndex == index && std::isfinite(target) && std::isfinite(current) &&
                std::abs(target - current) <= 0.000001f)
                continue;
            if (!setParameter(index, target, error)) return false;
        }
    }
    setBypassed(state.bypassed);
    return true;
}

std::optional<PluginStateSpec> PluginRack::persistedState(juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return std::nullopt;
    }
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
    return result;
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

bool equivalentDisplayText(const juce::String& input, const juce::String& output,
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

}  // namespace

std::optional<float> PluginRack::parameterValueForText(const int index, const juce::String& text,
                                                       juce::String& error) const {
    const auto& parameters = plugin->getParameters();
    if (index < 0 || index >= parameters.size() || parameters[index] == nullptr) {
        error = "Plugin parameter index is out of range.";
        return std::nullopt;
    }
    const auto* parameter = parameters[index];
    const auto value = parameter->getValueForText(text);
    if (text.trim().isEmpty() || !std::isfinite(value) || value < 0.0f || value > 1.0f ||
        !equivalentDisplayText(text, parameter->getText(value, 96), parameter->getLabel())) {
        error = "Plugin cannot convert this display text unambiguously.";
        return std::nullopt;
    }
    return value;
}

bool PluginRack::synchronizeParameterController(juce::String& error) {
    if (!parameterControllerSyncPending.load(std::memory_order_acquire)) return true;
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    try {
        // JUCE flushes pending VST3 Edit Controller updates at the state-capture
        // boundary. A completed audio block alone does not flush that dispatcher.
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        parameterControllerSyncPending.store(false, std::memory_order_release);
        return true;
    } catch (...) {
        error = "VST3 parameter controller synchronization failed.";
        return false;
    }
}

void PluginRack::updateParameterCount(juce::AudioProcessor& processor) {
    cachedParameterCount.store(static_cast<std::size_t>(processor.getParameters().size()),
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
    };
}

std::vector<PluginParameterInfo> PluginRack::parameters() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    std::vector<PluginParameterInfo> result;
    if (plugin == nullptr) return result;
    const auto& parameters = plugin->getParameters();
    result.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        auto* parameter = parameters[index];
        if (parameter == nullptr) continue;
        const auto value = parameter->getValue();
        const auto discrete = parameter->isDiscrete();
        const auto steps =
            discrete ? static_cast<std::uint32_t>(std::max(0, parameter->getNumSteps())) : 0u;
        PluginParameterInfo info{index,
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
