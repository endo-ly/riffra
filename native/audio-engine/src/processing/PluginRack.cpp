#include "PluginRack.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <exception>
#include <new>
#include <vector>

namespace riffra {

void PluginRack::PendingMidi::reset() {
    const juce::ScopedLock guard(lock);
    messages.clear();
    messages.ensureSize(2048);
}

void PluginRack::PendingMidi::add(const juce::MidiMessage& message) {
    const juce::ScopedLock guard(lock);
    messages.addEvent(message, 0);
}

void PluginRack::PendingMidi::appendTo(
    juce::MidiBuffer& destination,
    const int sampleCount) {
    const juce::ScopedLock guard(lock);
    for (const auto metadata : messages)
        destination.addEvent(
            metadata.getMessage(),
            juce::jlimit(0, std::max(0, sampleCount - 1), metadata.samplePosition));
    messages.clear();
}

namespace {

class AtomicFlagReset final {
public:
    explicit AtomicFlagReset(std::atomic<bool>& target) noexcept : flag(target) {}
    ~AtomicFlagReset() { flag.store(false, std::memory_order_release); }

private:
    std::atomic<bool>& flag;
};

juce::AudioProcessor::BusesLayout layoutWithMainBuses(juce::AudioProcessor& processor,
                                                      const juce::AudioChannelSet& input,
                                                      const juce::AudioChannelSet& output) {
    auto layout = processor.getBusesLayout();
    for (auto& bus : layout.inputBuses) bus = juce::AudioChannelSet::disabled();
    for (auto& bus : layout.outputBuses) bus = juce::AudioChannelSet::disabled();
    if (!layout.inputBuses.isEmpty()) layout.inputBuses.set(0, input);
    if (!layout.outputBuses.isEmpty()) layout.outputBuses.set(0, output);
    return layout;
}

}  // namespace

std::optional<PluginLoadError> PluginRack::load(const juce::String& path, const double sampleRate,
                                                const int blockSize) {
    mutationInProgress.store(true, std::memory_order_release);
    const AtomicFlagReset resetMutation(mutationInProgress);
    const juce::File file(path);
    if (path.isEmpty() || !file.exists()) {
        return PluginLoadError{
            "pluginPath",
            "VST3 bundle or file does not exist: " + path,
        };
    }
    if (formatManager.getNumFormats() == 0) juce::addDefaultFormatsToManager(formatManager);

    juce::VST3PluginFormat format;
    juce::OwnedArray<juce::PluginDescription> descriptions;
    format.findAllTypesForFile(descriptions, path);
    if (descriptions.isEmpty()) {
        return PluginLoadError{
            "pluginDescription",
            "No VST3 component could be described: " + path,
        };
    }

    juce::String instanceError;
    std::unique_ptr<juce::AudioPluginInstance> candidate;
    try {
        candidate = formatManager.createPluginInstance(*descriptions[0], sampleRate, blockSize,
                                                       instanceError);
    } catch (const std::exception& exception) {
        return PluginLoadError{
            "pluginInstance",
            "VST3 instance creation raised an exception: " + juce::String(exception.what()),
        };
    } catch (...) {
        return PluginLoadError{
            "pluginInstance",
            "VST3 instance creation failed with an unknown exception.",
        };
    }
    if (candidate == nullptr) {
        return PluginLoadError{
            "pluginInstance",
            instanceError.isNotEmpty() ? instanceError
                                       : "The VST3 host could not create a plugin instance.",
        };
    }

    if (auto configurationError = configureProcessor(*candidate, sampleRate, blockSize))
        return configurationError;

    updateParameterCache(*candidate);
    const auto candidateParameterCount = static_cast<std::size_t>(candidate->getParameters().size());
    pendingMidi.reset();
    const auto inputChannels = candidate->getMainBusNumInputChannels();
    const auto outputChannels = candidate->getMainBusNumOutputChannels();
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    juce::String parameterQueueError;
    if (!allocateParameterQueue(candidateParameterCount, parameterQueueError))
        return PluginLoadError { "parameterQueue", parameterQueueError };
    if (plugin != nullptr) {
        plugin->releaseResources();
        destroyCount.fetch_add(1, std::memory_order_relaxed);
    }
    plugin = std::move(candidate);
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath = path;
        pluginName = descriptions[0]->name;
    }
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    pluginInputChannels.store(inputChannels, std::memory_order_release);
    pluginOutputChannels.store(outputChannels, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
    panicPending.store(true, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    processedBlocks.store(0, std::memory_order_release);
    contentionBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
    loaded.store(true, std::memory_order_release);
    loadCount.fetch_add(1, std::memory_order_relaxed);
    return std::nullopt;
}

std::optional<PluginLoadError> PluginRack::configureProcessor(juce::AudioProcessor& processor,
                                                              const double sampleRate,
                                                              const int blockSize) {
    if (!std::isfinite(sampleRate) || sampleRate <= 0.0 || blockSize <= 0) {
        return PluginLoadError{
            "pluginInitialization",
            "VST3 initialization requires an active sample rate and block size.",
        };
    }
    if (processor.getBusCount(false) == 0) {
        return PluginLoadError{
            "pluginLayout",
            "The VST3 does not expose an audio output bus.",
        };
    }

    const bool hasInputBus = processor.getBusCount(true) > 0;
    std::vector<juce::AudioProcessor::BusesLayout> candidates;
    if (hasInputBus) {
        candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::stereo(),
                                                 juce::AudioChannelSet::stereo()));
        candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::mono(),
                                                 juce::AudioChannelSet::stereo()));
    } else {
        candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::disabled(),
                                                 juce::AudioChannelSet::stereo()));
    }
    const auto selected = std::find_if(
        candidates.begin(), candidates.end(),
        [&processor](const auto& layout) { return processor.checkBusesLayoutSupported(layout); });
    if (selected == candidates.end() || !processor.setBusesLayout(*selected)) {
        return PluginLoadError{
            "pluginLayout",
            hasInputBus
                ? "The VST3 supports neither stereo-to-stereo nor mono-to-stereo processing."
                : "The VST3 does not support stereo output.",
        };
    }

    try {
        processor.setNonRealtime(false);
        processor.setProcessingPrecision(juce::AudioProcessor::singlePrecision);
        processor.setRateAndBufferSizeDetails(sampleRate, blockSize);
        processor.prepareToPlay(sampleRate, blockSize);
        processor.reset();
    } catch (const std::exception& exception) {
        processor.releaseResources();
        return PluginLoadError{
            "pluginInitialization",
            "VST3 initialization raised an exception: " + juce::String(exception.what()),
        };
    } catch (...) {
        processor.releaseResources();
        return PluginLoadError{
            "pluginInitialization",
            "VST3 initialization failed with an unknown exception.",
        };
    }
    return std::nullopt;
}

void PluginRack::clear() noexcept {
    mutationInProgress.store(true, std::memory_order_release);
    const AtomicFlagReset resetMutation(mutationInProgress);
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) {
        plugin->releaseResources();
        destroyCount.fetch_add(1, std::memory_order_relaxed);
    }
    plugin.reset();
    pendingParameterValues.reset();
    pendingParameterDirty.reset();
    pendingParameterCapacity.store(0, std::memory_order_release);
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath.clear();
        pluginName.clear();
        cachedParameters.clear();
    }
    loaded.store(false, std::memory_order_release);
    pluginInputChannels.store(0, std::memory_order_release);
    pluginOutputChannels.store(0, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
    panicPending.store(true, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    processedBlocks.store(0, std::memory_order_release);
    contentionBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
}

void PluginRack::release() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->releaseResources();
}

void PluginRack::prepare(const double sampleRate, const int blockSize) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    if (sampleRate > 0.0)
        pendingMidi.reset();
    if (plugin != nullptr) {
        plugin->setRateAndBufferSizeDetails(sampleRate, blockSize);
        plugin->prepareToPlay(sampleRate, blockSize);
        plugin->reset();
        if (pendingParameterCapacity.load(std::memory_order_acquire)
            != static_cast<std::size_t>(plugin->getParameters().size())) {
            juce::String ignored;
            (void) allocateParameterQueue(
                static_cast<std::size_t>(plugin->getParameters().size()), ignored);
        }
    }
}

void PluginRack::reset() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->reset();
    panicPending.store(true, std::memory_order_release);
}

void PluginRack::setBypassed(const bool shouldBypass) noexcept {
    bypassed.store(shouldBypass, std::memory_order_release);
}

void PluginRack::addProcessorListener(juce::AudioProcessorListener& listener) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->addListener(&listener);
}

void PluginRack::removeProcessorListener(juce::AudioProcessorListener& listener) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->removeListener(&listener);
}

void PluginRack::enqueueParameterChange(const int index, const float value) noexcept {
    const auto capacity = pendingParameterCapacity.load(std::memory_order_acquire);
    if (index < 0 || static_cast<std::size_t>(index) >= capacity
        || pendingParameterValues == nullptr || pendingParameterDirty == nullptr)
        return;
    const auto offset = static_cast<std::size_t>(index);
    pendingParameterValues[offset].store(juce::jlimit(0.0f, 1.0f, value), std::memory_order_release);
    pendingParameterDirty[offset].store(true, std::memory_order_release);
}

std::size_t PluginRack::parameterCount() const noexcept {
    const juce::ScopedLock statusGuard(statusLock);
    return cachedParameters.size();
}

bool PluginRack::allocateParameterQueue(
    const std::size_t count,
    juce::String& error) noexcept {
    if (count == 0) {
        pendingParameterValues.reset();
        pendingParameterDirty.reset();
        pendingParameterCapacity.store(0, std::memory_order_release);
        return true;
    }
    auto values = std::unique_ptr<std::atomic<float>[]>(new (std::nothrow) std::atomic<float>[count]);
    auto dirty = std::unique_ptr<std::atomic<bool>[]>(new (std::nothrow) std::atomic<bool>[count]);
    if (values == nullptr || dirty == nullptr) {
        error = "The plugin parameter change queue could not be allocated.";
        return false;
    }
    for (std::size_t index = 0; index < count; ++index) {
        values[index].store(0.0f, std::memory_order_relaxed);
        dirty[index].store(false, std::memory_order_relaxed);
    }
    pendingParameterValues = std::move(values);
    pendingParameterDirty = std::move(dirty);
    pendingParameterCapacity.store(count, std::memory_order_release);
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

bool PluginRack::applyPersistedState(const juce::var& state, juce::String& error) noexcept {
    if (!state.isObject()) {
        error = "Plugin persisted state must be an object.";
        return false;
    }
    const auto stateData = state.getProperty("stateData", {}).toString();
    if (stateData.isNotEmpty()) {
        if (!setState(stateData, error))
            return false;
    }
    const auto values = state.getProperty("parameterValues", {});
    if (values.isArray()) {
        const auto available = parameterStatus().getProperty("parameters", {});
        const auto count = available.isArray() ? available.size() : 0;
        for (int index = 0; index < std::min(values.size(), count); ++index)
            if (!setParameter(index, static_cast<float>(values[index]), error))
                return false;
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
    for (auto* parameter : plugin->getParameters())
        values.add(parameter != nullptr ? parameter->getValue() : 0.0f);
    result->setProperty("parameterValues", values);
    result->setProperty("bypassed", bypassed.load(std::memory_order_acquire));
    try {
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        result->setProperty(
            "stateData",
            state.isEmpty()
                ? juce::String()
                : juce::Base64::toBase64(state.getData(), state.getSize()));
    } catch (const std::exception& exception) {
        error = "VST3 state capture raised an exception: " + juce::String(exception.what());
        return {};
    } catch (...) {
        error = "VST3 state capture failed with an unknown exception.";
        return {};
    }
    return juce::var(result);
}

void PluginRack::enqueueMidi(const juce::MidiMessage& message) noexcept {
    if (!loaded.load(std::memory_order_acquire))
        return;
    pendingMidi.add(message);
}

void PluginRack::allNotesOff() noexcept {
    panicPending.store(true, std::memory_order_release);
}

bool PluginRack::isLoaded() const noexcept {
    return loaded.load(std::memory_order_acquire);
}

bool PluginRack::isInstrument() const noexcept {
    return loaded.load(std::memory_order_acquire)
        && pluginInputChannels.load(std::memory_order_acquire) == 0;
}

int PluginRack::latencySamples() const noexcept {
    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked() || plugin == nullptr) return 0;
    return std::max(0, plugin->getLatencySamples());
}

int PluginRack::tailSamples() const noexcept {
    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked() || plugin == nullptr)
        return 0;
    const auto seconds = plugin->getTailLengthSeconds();
    const auto sampleRate = preparedSampleRate.load(std::memory_order_acquire);
    if (!std::isfinite(seconds) || seconds <= 0.0 || sampleRate <= 0.0)
        return 0;
    return static_cast<int>(std::min(
        sampleRate * 30.0,
        std::ceil(seconds * sampleRate)));
}

void PluginRack::process(const float* const* inputChannelData, const int numInputChannels,
                         float* const* outputChannelData, const int numOutputChannels,
                         const int numSamples,
                         const juce::MidiBuffer* const timelineMidi) noexcept {
    for (int channel = 0; channel < numOutputChannels; ++channel) {
        auto* output = outputChannelData[channel];
        if (output == nullptr) continue;
        const auto inputIndex = numInputChannels == 1 ? 0 : channel;
        const auto* input = inputIndex < numInputChannels ? inputChannelData[inputIndex] : nullptr;
        if (input != nullptr)
            juce::FloatVectorOperations::copy(output, input, numSamples);
        else
            juce::FloatVectorOperations::clear(output, numSamples);
    }

    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked()) {
        if (loaded.load(std::memory_order_acquire) ||
            mutationInProgress.load(std::memory_order_acquire)) {
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
    if (plugin == nullptr || numOutputChannels <= 0 || numSamples <= 0) return;
    applyQueuedParameterChanges();
    if (bypassed.load(std::memory_order_acquire)) {
        bypassedBlocks.fetch_add(1, std::memory_order_relaxed);
        return;
    }

    const auto requiredOutputs = pluginOutputChannels.load(std::memory_order_acquire);
    for (int channel = requiredOutputs; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);

    juce::AudioBuffer<float> buffer(outputChannelData, numOutputChannels, numSamples);
    juce::MidiBuffer midi;
    if (timelineMidi != nullptr) {
        for (const auto metadata : *timelineMidi) {
            midi.addEvent(metadata.getMessage(), metadata.samplePosition);
        }
    }
    pendingMidi.appendTo(midi, numSamples);
    if (panicPending.exchange(false, std::memory_order_acq_rel)) {
        for (int channel = 1; channel <= 16; ++channel)
            midi.addEvent(juce::MidiMessage::allNotesOff(channel), 0);
    }
    plugin->processBlock(buffer, midi);
    processedBlocks.fetch_add(1, std::memory_order_relaxed);
}

void PluginRack::applyQueuedParameterChanges() noexcept {
    if (plugin == nullptr)
        return;
    const auto& parameters = plugin->getParameters();
    const auto count = std::min(
        parameters.size(), static_cast<int>(pendingParameterCapacity.load(std::memory_order_acquire)));
    for (int index = 0; index < count; ++index) {
        if (!pendingParameterDirty[static_cast<std::size_t>(index)].exchange(
                false, std::memory_order_acq_rel))
            continue;
        if (auto* parameter = parameters[index])
            parameter->setValueNotifyingHost(
                pendingParameterValues[static_cast<std::size_t>(index)].load(
                    std::memory_order_acquire));
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
    result->setProperty("contentionBlocks",
                        static_cast<juce::int64>(contentionBlocks.load(std::memory_order_acquire)));
    result->setProperty("transitionBlocks",
                        static_cast<juce::int64>(transitionBlocks.load(std::memory_order_acquire)));
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

}  // namespace riffra
