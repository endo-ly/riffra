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
    const auto parameters = parameterStatus().getProperty("parameters", {});
    if (!parameters.isArray() || index < 0 || index >= parameters.size()) {
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

bool PluginRack::applyPersistedState(const juce::var& state, juce::String& error) noexcept {
    if (!state.isObject()) {
        error = "Plugin persisted state must be an object.";
        return false;
    }
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
    const auto stateData = state.getProperty("stateData", {}).toString();
    if (stateData.isNotEmpty()) {
        if (!applyStateData(stateData, error)) return false;
    }
    const auto values = state.getProperty("parameterValues", {});
    if (values.isArray()) {
        const auto available = parameterStatus().getProperty("parameters", {});
        const auto count = available.isArray() ? available.size() : 0;
        for (int index = 0; index < std::min(values.size(), count); ++index) {
            const auto target = static_cast<float>(values[index]);
            const auto availableIndex = static_cast<int>(available[index].getProperty("index", -1));
            const auto current = static_cast<float>(available[index].getProperty("value", target));
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
    setBypassed(static_cast<bool>(state.getProperty("bypassed", false)));
    return true;
}

juce::var PluginRack::persistedState(juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return {};
    }
    auto* result = new juce::DynamicObject();
    juce::Array<juce::var> values;
    std::vector<CachedParameter> cached;
    {
        const juce::ScopedLock statusGuard(statusLock);
        cached = cachedParameters;
    }
    const auto parameters = plugin->getParameters();
    for (int index = 0; index < parameters.size(); ++index) {
        const auto found =
            std::find_if(cached.begin(), cached.end(),
                         [index](const auto& parameter) { return parameter.index == index; });
        values.add(found != cached.end()
                       ? found->value
                       : (parameters[index] != nullptr ? parameters[index]->getValue() : 0.0f));
    }
    result->setProperty("parameterValues", values);
    result->setProperty("bypassed", bypassed.load(std::memory_order_acquire));
    try {
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        result->setProperty("stateData", state.isEmpty() ? juce::String()
                                                         : juce::Base64::toBase64(state.getData(),
                                                                                  state.getSize()));
    } catch (const std::exception& exception) {
        error = "VST3 state capture raised an exception: " + juce::String(exception.what());
        return {};
    } catch (...) {
        error = "VST3 state capture failed with an unknown exception.";
        return {};
    }
    return juce::var(result);
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
    std::vector<CachedParameter> next;
    const auto& parameters = processor.getParameters();
    next.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        auto* parameter = parameters[index];
        if (parameter == nullptr) continue;
        next.push_back(CachedParameter{
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

juce::var PluginRack::cachedStatus(const bool includeParameters) const {
    const juce::ScopedLock lock(statusLock);
    auto* result = new juce::DynamicObject();
    result->setProperty("loaded", loaded.load(std::memory_order_acquire));
    result->setProperty("path", pluginPath);
    result->setProperty("name", pluginName);
    result->setProperty("bypassed", bypassed.load(std::memory_order_acquire));
    result->setProperty("sampleRate", preparedSampleRate.load(std::memory_order_acquire));
    result->setProperty("blockSize", preparedBlockSize.load(std::memory_order_acquire));
    result->setProperty("inputChannels", pluginInputChannels.load(std::memory_order_acquire));
    result->setProperty("outputChannels", pluginOutputChannels.load(std::memory_order_acquire));
    result->setProperty("bypassedBlocks",
                        static_cast<juce::int64>(bypassedBlocks.load(std::memory_order_acquire)));
    result->setProperty("processedBlocks",
                        static_cast<juce::int64>(processedBlocks.load(std::memory_order_acquire)));
    result->setProperty("transitionBlocks",
                        static_cast<juce::int64>(transitionBlocks.load(std::memory_order_acquire)));
    result->setProperty("droppedMidiEvents", static_cast<juce::int64>(pendingMidi.droppedEvents()));
    result->setProperty("loadCount",
                        static_cast<juce::int64>(loadCount.load(std::memory_order_acquire)));
    result->setProperty("destroyCount",
                        static_cast<juce::int64>(destroyCount.load(std::memory_order_acquire)));
    if (includeParameters) {
        juce::Array<juce::var> parameters;
        for (const auto& parameter : cachedParameters) {
            auto* item = new juce::DynamicObject();
            item->setProperty("index", parameter.index);
            item->setProperty("name", parameter.name);
            item->setProperty("value", parameter.value);
            item->setProperty("defaultValue", parameter.defaultValue);
            item->setProperty("automatable", parameter.automatable);
            parameters.add(juce::var(item));
        }
        result->setProperty("parameters", parameters);
    }
    return juce::var(result);
}

juce::var PluginRack::status() const { return cachedStatus(false); }

juce::var PluginRack::parameterStatus() const { return cachedStatus(true); }

juce::var PluginRack::programStatus() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    auto* result = new juce::DynamicObject();
    result->setProperty("supported", false);
    result->setProperty("currentIndex", -1);
    result->setProperty("currentName", juce::String());
    result->setProperty("programs", juce::Array<juce::var>{});
    if (plugin == nullptr) return juce::var(result);

    try {
        const auto programCount = plugin->getNumPrograms();
        juce::Array<juce::var> programs;
        for (int index = 0; index < programCount; ++index) {
            auto* program = new juce::DynamicObject();
            program->setProperty("index", index);
            program->setProperty("name", plugin->getProgramName(index));
            programs.add(juce::var(program));
        }
        const auto currentIndex = plugin->getCurrentProgram();
        result->setProperty("supported", programCount > 0);
        result->setProperty("currentIndex", currentIndex);
        result->setProperty("currentName", currentIndex >= 0 && currentIndex < programCount
                                               ? plugin->getProgramName(currentIndex)
                                               : juce::String());
        result->setProperty("programs", programs);
    } catch (const std::exception& exception) {
        result->setProperty("error",
                            "VST3 program enumeration failed: " + juce::String(exception.what()));
    } catch (...) {
        result->setProperty("error", "VST3 program enumeration failed.");
    }
    return juce::var(result);
}

bool PluginRack::hasEditor() const noexcept {
    return cachedHasEditor.load(std::memory_order_acquire);
}

}  // namespace riffra
