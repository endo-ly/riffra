#include "PluginRack.h"

namespace riffra {

namespace {

class AtomicFlagReset final {
public:
    explicit AtomicFlagReset(std::atomic<bool>& target) noexcept : flag(target) {}
    ~AtomicFlagReset() { flag.store(false, std::memory_order_release); }

private:
    std::atomic<bool>& flag;
};

}

bool PluginRack::load(
    const juce::String& path,
    const double sampleRate,
    const int blockSize,
    juce::String& error) {
    mutationInProgress.store(true, std::memory_order_release);
    const AtomicFlagReset resetMutation(mutationInProgress);
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    const juce::File file(path);
    if (path.isEmpty() || !file.exists()) {
        error = "VST3 path does not exist: " + path;
        return false;
    }
    if (formatManager.getNumFormats() == 0)
        juce::addHeadlessDefaultFormatsToManager(formatManager);

    juce::VST3PluginFormat format;
    juce::OwnedArray<juce::PluginDescription> descriptions;
    format.findAllTypesForFile(descriptions, path);
    if (descriptions.isEmpty()) {
        error = "No VST3 component could be described: " + path;
        return false;
    }

    auto candidate = formatManager.createPluginInstance(
        *descriptions[0],
        sampleRate,
        blockSize,
        error);
    if (candidate == nullptr)
        return false;

    candidate->prepareToPlay(sampleRate, blockSize);
    updateParameterCache(*candidate);
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath = path;
        pluginName = descriptions[0]->name;
    }
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    plugin = std::move(candidate);
    bypassed.store(false, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    contentionBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
    loaded.store(true, std::memory_order_release);
    return true;
}

void PluginRack::clear() noexcept {
    mutationInProgress.store(true, std::memory_order_release);
    const AtomicFlagReset resetMutation(mutationInProgress);
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->releaseResources();
    plugin.reset();
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath.clear();
        pluginName.clear();
        cachedParameters.clear();
    }
    loaded.store(false, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
}

void PluginRack::release() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->releaseResources();
}

void PluginRack::prepare(const double sampleRate, const int blockSize) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    if (plugin != nullptr)
        plugin->prepareToPlay(sampleRate, blockSize);
}

void PluginRack::setBypassed(const bool shouldBypass) noexcept {
    bypassed.store(shouldBypass, std::memory_order_release);
}

bool PluginRack::setParameter(const int index, const float value, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin == nullptr) {
        error = "No VST3 plugin is loaded.";
        return false;
    }
    const auto& parameters = plugin->getParameters();
    if (index < 0 || index >= parameters.size() || parameters[index] == nullptr) {
        error = "Plugin parameter index is out of range.";
        return false;
    }
    parameters[index]->setValueNotifyingHost(juce::jlimit(0.0f, 1.0f, value));
    updateParameterCache(*plugin);
    return true;
}

bool PluginRack::setState(const juce::String& base64, juce::String& error) noexcept {
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

void PluginRack::process(
    const float* const* inputChannelData,
    const int numInputChannels,
    float* const* outputChannelData,
    const int numOutputChannels,
    const int numSamples) noexcept {
    for (int channel = 0; channel < numOutputChannels; ++channel) {
        auto* output = outputChannelData[channel];
        if (output == nullptr)
            continue;
        const auto inputIndex = numInputChannels == 1 ? 0 : channel;
        const auto* input = inputIndex < numInputChannels ? inputChannelData[inputIndex] : nullptr;
        if (input != nullptr)
            juce::FloatVectorOperations::copy(output, input, numSamples);
        else
            juce::FloatVectorOperations::clear(output, numSamples);
    }

    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked()) {
        if (loaded.load(std::memory_order_acquire)
            || mutationInProgress.load(std::memory_order_acquire)) {
            for (int channel = 0; channel < numOutputChannels; ++channel)
                if (outputChannelData[channel] != nullptr)
                    juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
            if (mutationInProgress.load(std::memory_order_acquire))
                transitionBlocks.fetch_add(1, std::memory_order_relaxed);
            else
                contentionBlocks.fetch_add(1, std::memory_order_relaxed);
        }
        return;
    }
    if (mutationInProgress.load(std::memory_order_acquire)) {
        for (int channel = 0; channel < numOutputChannels; ++channel)
            if (outputChannelData[channel] != nullptr)
                juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);
        transitionBlocks.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (plugin == nullptr || numOutputChannels <= 0 || numSamples <= 0)
        return;
    if (bypassed.load(std::memory_order_acquire)) {
        bypassedBlocks.fetch_add(1, std::memory_order_relaxed);
        return;
    }

    juce::AudioBuffer<float> buffer(outputChannelData, numOutputChannels, numSamples);
    juce::MidiBuffer midi;
    plugin->processBlock(buffer, midi);
}

void PluginRack::updateParameterCache(juce::AudioProcessor& processor) {
    std::vector<CachedParameter> next;
    const auto& parameters = processor.getParameters();
    next.reserve(static_cast<std::size_t>(parameters.size()));
    for (int index = 0; index < parameters.size(); ++index) {
        auto* parameter = parameters[index];
        if (parameter == nullptr)
            continue;
        next.push_back(CachedParameter {
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

juce::var PluginRack::cachedStatus() const {
    const juce::ScopedLock lock(statusLock);
    auto* result = new juce::DynamicObject();
    result->setProperty("loaded", loaded.load(std::memory_order_acquire));
    result->setProperty("path", pluginPath);
    result->setProperty("name", pluginName);
    result->setProperty("bypassed", bypassed.load(std::memory_order_acquire));
    result->setProperty("sampleRate", preparedSampleRate.load(std::memory_order_acquire));
    result->setProperty("blockSize", preparedBlockSize.load(std::memory_order_acquire));
    result->setProperty(
        "bypassedBlocks",
        static_cast<juce::int64>(bypassedBlocks.load(std::memory_order_acquire)));
    result->setProperty(
        "contentionBlocks",
        static_cast<juce::int64>(contentionBlocks.load(std::memory_order_acquire)));
    result->setProperty(
        "transitionBlocks",
        static_cast<juce::int64>(transitionBlocks.load(std::memory_order_acquire)));
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
    return juce::var(result);
}

juce::var PluginRack::status() const {
    return cachedStatus();
}

juce::var PluginRack::completeStatus() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    auto result = cachedStatus();
    if (plugin != nullptr) {
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        result.getDynamicObject()->setProperty("stateData", state.toBase64Encoding());
    }
    return result;
}

} // namespace riffra
