#include <algorithm>
#include <cmath>
#include <new>
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

void PluginRack::enqueueParameterChange(const int index, const float value) noexcept {
    activeReaders.fetch_add(1, std::memory_order_acq_rel);
    auto* queue = activeParameterQueue.load(std::memory_order_acquire);
    if (queue == nullptr || index < 0 || static_cast<std::size_t>(index) >= queue->capacity) {
        activeReaders.fetch_sub(1, std::memory_order_release);
        return;
    }
    const auto offset = static_cast<std::size_t>(index);
    const auto normalized = juce::jlimit(0.0f, 1.0f, value);
    queue->values[offset].store(normalized, std::memory_order_release);
    queue->dirty[offset].store(true, std::memory_order_release);
    const juce::ScopedLock statusGuard(statusLock);
    const auto cached =
        std::find_if(cachedParameters.begin(), cachedParameters.end(),
                     [index](const auto& parameter) { return parameter.index == index; });
    if (cached != cachedParameters.end()) cached->value = normalized;
    activeReaders.fetch_sub(1, std::memory_order_release);
}

std::size_t PluginRack::parameterCount() const noexcept {
    const juce::ScopedLock statusGuard(statusLock);
    return cachedParameters.size();
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
    if (parameterQueue != nullptr) retiredParameterQueues.push_back(std::move(parameterQueue));
    parameterQueue = std::move(candidate);
    activeParameterQueue.store(parameterQueue.get(), std::memory_order_release);
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
        updateParameterCache(*plugin);
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
    updateParameterCache(*plugin);
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
            const auto current = available[offset].value;
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
    std::vector<PluginParameterInfo> cached;
    {
        const juce::ScopedLock statusGuard(statusLock);
        cached = cachedParameters;
    }
    const auto parameters = plugin->getParameters();
    result.parameterValues.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        const auto found =
            std::find_if(cached.begin(), cached.end(),
                         [index](const auto& parameter) { return parameter.index == index; });
        result.parameterValues.push_back(
            found != cached.end()
                ? found->value
                : (parameters[index] != nullptr ? parameters[index]->getValue() : 0.0f));
    }
    result.bypassed = bypassed.load(std::memory_order_acquire);
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
    return result;
}

void PluginRack::applyQueuedParameterChanges(juce::AudioProcessor* const processor,
                                             ParameterQueue* const queue) noexcept {
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

void PluginRack::updateParameterCache(juce::AudioProcessor& processor) {
    std::vector<PluginParameterInfo> next;
    const auto& parameters = processor.getParameters();
    next.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        auto* parameter = parameters[index];
        if (parameter == nullptr) continue;
        next.push_back(PluginParameterInfo{
            index,
            parameter->getName(96),
            parameter->getValue(),
            parameter->getDefaultValue(),
            parameter->isAutomatable(),
        });
    }
    const juce::ScopedLock lock(statusLock);
    cachedParameters = std::move(next);
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
    const juce::ScopedLock lock(statusLock);
    return cachedParameters;
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
