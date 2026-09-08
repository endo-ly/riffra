#include "PluginRack.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <exception>
#include <limits>
#include <new>
#include <vector>

#include "FaultInjection.h"

namespace riffra {

PluginRack::PluginRack() {
    // Reserve enough raw storage for the entire bounded live-MIDI queue before
    // the rack can be used by the audio callback.
    // JUCE stores each event as timestamp (int32), payload length (uint16), and
    // payload bytes. Reserve the sum of the per-source event limits up front so
    // process() never grows this buffer on the audio callback.
    constexpr auto maximumMidiEvents = PendingMidi::kCapacity + kMaximumPanicMidiEvents;
    processMidi.ensureSize(maximumMidiEvents *
                           (PendingMidi::kMaximumMessageBytes + kMidiEventOverhead));
}

void PluginRack::PendingMidi::reset() {
    Event ignored;
    while (messages.tryPopNonRealtime(ignored)) {
    }
}

bool PluginRack::PendingMidi::add(const juce::MidiMessage& message) noexcept {
    const auto size = message.getRawDataSize();
    if (size <= 0 || static_cast<std::size_t>(size) > kMaximumMessageBytes) {
        droppedEventsCount.fetch_add(1, std::memory_order_relaxed);
        return false;
    }
    Event event;
    event.size = static_cast<std::uint16_t>(size);
    std::copy_n(message.getRawData(), size, event.bytes.begin());
    return messages.tryPush(event);
}

void PluginRack::PendingMidi::appendTo(juce::MidiBuffer& destination,
                                       const int sampleCount) noexcept {
    Event event;
    const auto sample = juce::jlimit(0, std::max(0, sampleCount - 1), 0);
    while (messages.tryPop(event))
        if (!destination.addEvent(event.bytes.data(), event.size, sample)) recordDropped();
}

void PluginRack::PendingMidi::recordDropped() noexcept {
    droppedEventsCount.fetch_add(1, std::memory_order_relaxed);
}

std::uint64_t PluginRack::PendingMidi::droppedEvents() const noexcept {
    return messages.droppedPushes() + droppedEventsCount.load(std::memory_order_acquire);
}

namespace {

struct PluginDescriptionCacheEntry final {
    juce::String path;
    juce::Time lastModificationTime;
    int64_t fileSize = 0;
    juce::PluginDescription description;
};

class PluginDescriptionCache final {
public:
    std::optional<juce::PluginDescription> findOrDescribe(const juce::File& file,
                                                          const juce::String& path) {
        FaultInjection::before(FaultStage::discovery);
        const auto lastModificationTime = file.getLastModificationTime();
        const auto fileSize = file.getSize();
        {
            const juce::ScopedLock guard(lock);
            const auto found = std::find_if(entries.begin(), entries.end(), [&](const auto& entry) {
                return entry.path == path && entry.lastModificationTime == lastModificationTime &&
                       entry.fileSize == fileSize;
            });
            if (found != entries.end()) return found->description;
        }

        juce::VST3PluginFormat format;
        juce::OwnedArray<juce::PluginDescription> descriptions;
        format.findAllTypesForFile(descriptions, path);
        if (descriptions.isEmpty()) return std::nullopt;

        PluginDescriptionCacheEntry entry{
            path,
            lastModificationTime,
            fileSize,
            *descriptions[0],
        };
        {
            const juce::ScopedLock guard(lock);
            entries.erase(std::remove_if(entries.begin(), entries.end(),
                                         [&](const auto& current) { return current.path == path; }),
                          entries.end());
            entries.push_back(entry);
        }
        return entry.description;
    }

private:
    juce::CriticalSection lock;
    std::vector<PluginDescriptionCacheEntry> entries;
};

PluginDescriptionCache& pluginDescriptionCache() {
    static PluginDescriptionCache cache;
    return cache;
}

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
    const juce::File file(path);
    if (path.isEmpty() || !file.exists()) {
        return PluginLoadError{
            "pluginPath",
            "VST3 bundle or file does not exist: " + path,
        };
    }
    if (formatManager.getNumFormats() == 0) juce::addDefaultFormatsToManager(formatManager);

    const auto description = pluginDescriptionCache().findOrDescribe(file, path);
    if (!description.has_value()) {
        return PluginLoadError{
            "pluginDescription",
            "No VST3 component could be described: " + path,
        };
    }

    juce::String instanceError;
    std::unique_ptr<juce::AudioPluginInstance> candidate;
    try {
        FaultInjection::before(FaultStage::create);
        candidate =
            formatManager.createPluginInstance(*description, sampleRate, blockSize, instanceError);
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

    auto candidateProgramCount = 0;
    try {
        candidateProgramCount = std::max(0, candidate->getNumPrograms());
    } catch (...) {
        // Program enumeration remains available through programStatus(), which
        // reports the plugin error. A failed capability probe must not prevent
        // the plugin from loading.
    }
    auto candidateHasEditor = false;
    try {
        candidateHasEditor = candidate->hasEditor();
    } catch (...) {
        // Editor creation remains available through createEditor(), which
        // reports the plugin error. A failed capability probe must not prevent
        // the plugin from loading.
    }
    updateParameterCache(*candidate);
    const auto candidateParameterCount =
        static_cast<std::size_t>(candidate->getParameters().size());
    pendingMidi.reset();
    const auto inputChannels = candidate->getMainBusNumInputChannels();
    const auto outputChannels = candidate->getMainBusNumOutputChannels();
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    juce::String parameterQueueError;
    if (!allocateParameterQueue(candidateParameterCount, parameterQueueError))
        return PluginLoadError{"parameterQueue", parameterQueueError};
    if (plugin != nullptr) retiredPlugins.push_back(std::move(plugin));
    plugin = std::move(candidate);
    activePlugin.store(plugin.get(), std::memory_order_release);
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath = path;
        pluginName = description->name;
    }
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    pluginInputChannels.store(inputChannels, std::memory_order_release);
    pluginOutputChannels.store(outputChannels, std::memory_order_release);
    cachedProgramCount.store(candidateProgramCount, std::memory_order_release);
    cachedHasEditor.store(candidateHasEditor, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
    panicPending.store(true, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    processedBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
    loaded.store(true, std::memory_order_release);
    loadCount.fetch_add(1, std::memory_order_relaxed);
    reclaimRetiredPlugins();
    return std::nullopt;
}

PluginRack::~PluginRack() {
    clear();
    activePlugin.store(nullptr, std::memory_order_release);
    activeParameterQueue.store(nullptr, std::memory_order_release);
    retiredPlugins.clear();
    retiredParameterQueues.clear();
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
        FaultInjection::before(FaultStage::prepare);
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
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) retiredPlugins.push_back(std::move(plugin));
    if (parameterQueue != nullptr) retiredParameterQueues.push_back(std::move(parameterQueue));
    activePlugin.store(nullptr, std::memory_order_release);
    activeParameterQueue.store(nullptr, std::memory_order_release);
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath.clear();
        pluginName.clear();
        cachedParameters.clear();
    }
    loaded.store(false, std::memory_order_release);
    pluginInputChannels.store(0, std::memory_order_release);
    pluginOutputChannels.store(0, std::memory_order_release);
    cachedProgramCount.store(0, std::memory_order_release);
    cachedHasEditor.store(false, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
    panicPending.store(true, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    processedBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
    reclaimRetiredPlugins();
}

void PluginRack::reclaimRetiredPlugins() noexcept {
    if (activeReaders.load(std::memory_order_acquire) != 0) return;
    for (auto& retired : retiredPlugins) {
        if (retired == nullptr) continue;
        try {
            FaultInjection::before(FaultStage::destroy);
        } catch (...) {
            continue;
        }
        retired->releaseResources();
        destroyCount.fetch_add(1, std::memory_order_relaxed);
    }
    retiredPlugins.clear();
    retiredParameterQueues.clear();
}

void PluginRack::release() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->releaseResources();
}

void PluginRack::prepare(const double sampleRate, const int blockSize) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    if (sampleRate > 0.0) pendingMidi.reset();
    if (plugin != nullptr) {
        plugin->setRateAndBufferSizeDetails(sampleRate, blockSize);
        plugin->prepareToPlay(sampleRate, blockSize);
        plugin->reset();
        const auto* queue = activeParameterQueue.load(std::memory_order_acquire);
        if (queue == nullptr || queue->capacity !=
            static_cast<std::size_t>(plugin->getParameters().size())) {
            juce::String ignored;
            (void)allocateParameterQueue(static_cast<std::size_t>(plugin->getParameters().size()),
                                         ignored);
        }
    }
}

void PluginRack::reset() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->reset();
    panicPending.store(true, std::memory_order_release);
}

void PluginRack::setBypassed(const bool shouldBypass) noexcept {
    bypassed.store(shouldBypass, std::memory_order_release);
}

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
    const auto cached = std::find_if(cachedParameters.begin(), cachedParameters.end(),
                                     [index](const auto& parameter) {
                                         return parameter.index == index;
                                     });
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
        const auto found = std::find_if(cached.begin(), cached.end(),
                                        [index](const auto& parameter) {
                                            return parameter.index == index;
                                        });
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

bool PluginRack::enqueueMidi(const juce::MidiMessage& message) noexcept {
    if (!loaded.load(std::memory_order_acquire)) return false;
    return pendingMidi.add(message);
}

bool PluginRack::prepareTimelineMidiCapacity(const std::size_t eventCapacity,
                                             juce::String& error) noexcept {
    constexpr auto bytesPerEvent = PendingMidi::kMaximumMessageBytes + kMidiEventOverhead;
    if (eventCapacity > (std::numeric_limits<int>::max() / bytesPerEvent) - PendingMidi::kCapacity -
                            kMaximumPanicMidiEvents) {
        error = "Timeline MIDI requires an audio buffer larger than the native runtime allows.";
        return false;
    }
    processMidi.ensureSize(static_cast<int>(
        (eventCapacity + PendingMidi::kCapacity + kMaximumPanicMidiEvents) * bytesPerEvent));
    return true;
}

void PluginRack::allNotesOff() noexcept { panicPending.store(true, std::memory_order_release); }

bool PluginRack::isLoaded() const noexcept { return loaded.load(std::memory_order_acquire); }

bool PluginRack::isInstrument() const noexcept {
    return loaded.load(std::memory_order_acquire) &&
           pluginInputChannels.load(std::memory_order_acquire) == 0;
}

int PluginRack::latencySamples() const noexcept {
    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked() || plugin == nullptr) return 0;
    return std::max(0, plugin->getLatencySamples());
}

int PluginRack::tailSamples() const noexcept {
    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked() || plugin == nullptr) return 0;
    const auto seconds = plugin->getTailLengthSeconds();
    const auto sampleRate = preparedSampleRate.load(std::memory_order_acquire);
    if (!std::isfinite(seconds) || seconds <= 0.0 || sampleRate <= 0.0) return 0;
    return static_cast<int>(std::min(sampleRate * 30.0, std::ceil(seconds * sampleRate)));
}

std::uint64_t PluginRack::droppedMidiEvents() const noexcept { return pendingMidi.droppedEvents(); }

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

    activeReaders.fetch_add(1, std::memory_order_acq_rel);
    const auto leave = [this] { activeReaders.fetch_sub(1, std::memory_order_release); };
    auto* active = activePlugin.load(std::memory_order_acquire);
    auto* queue = activeParameterQueue.load(std::memory_order_acquire);
    if (active == nullptr || numOutputChannels <= 0 || numSamples <= 0) {
        leave();
        return;
    }
    applyQueuedParameterChanges(active, queue);
    if (bypassed.load(std::memory_order_acquire)) {
        bypassedBlocks.fetch_add(1, std::memory_order_relaxed);
        leave();
        return;
    }

    const auto requiredOutputs = pluginOutputChannels.load(std::memory_order_acquire);
    for (int channel = requiredOutputs; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);

    juce::AudioBuffer<float> buffer(outputChannelData, numOutputChannels, numSamples);
    processMidi.clear();
    if (panicPending.exchange(false, std::memory_order_acq_rel)) {
        for (int channel = 1; channel <= 16; ++channel) {
            if (!processMidi.addEvent(juce::MidiMessage::allNotesOff(channel), 0))
                pendingMidi.recordDropped();
            if (!processMidi.addEvent(juce::MidiMessage::allSoundOff(channel), 0))
                pendingMidi.recordDropped();
            if (!processMidi.addEvent(juce::MidiMessage::controllerEvent(channel, 64, 0), 0))
                pendingMidi.recordDropped();
        }
    }
    if (timelineMidi != nullptr) {
        for (const auto metadata : *timelineMidi) {
            if (metadata.data == nullptr || metadata.numBytes <= 0 ||
                static_cast<std::size_t>(metadata.numBytes) > PendingMidi::kMaximumMessageBytes) {
                pendingMidi.recordDropped();
                continue;
            }
            if (!processMidi.addEvent(metadata.data, metadata.numBytes, metadata.samplePosition)) {
                pendingMidi.recordDropped();
                continue;
            }
        }
    }
    pendingMidi.appendTo(processMidi, numSamples);
    active->processBlock(buffer, processMidi);
    processedBlocks.fetch_add(1, std::memory_order_relaxed);
    leave();
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
