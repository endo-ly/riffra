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
                                                      const juce::AudioChannelSet& output,
                                                      const int auxBusIndex,
                                                      const int auxChannels) {
    auto layout = processor.getBusesLayout();
    for (auto& bus : layout.inputBuses) bus = juce::AudioChannelSet::disabled();
    for (auto& bus : layout.outputBuses) bus = juce::AudioChannelSet::disabled();
    if (!layout.inputBuses.isEmpty()) layout.inputBuses.set(0, input);
    if (!layout.outputBuses.isEmpty()) layout.outputBuses.set(0, output);
    if (auxBusIndex >= 0 && auxChannels > 0 && auxBusIndex < layout.inputBuses.size())
        layout.inputBuses.set(auxBusIndex, auxChannels == 1 ? juce::AudioChannelSet::mono()
                                                            : juce::AudioChannelSet::stereo());
    return layout;
}

}  // namespace

std::optional<PluginLoadError> PluginRack::load(const juce::String& path, const double sampleRate,
                                                const int blockSize,
                                                const PluginProcessingMode mode,
                                                const bool sidechainInput) {
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

    candidate->setPlayHead(&playHead);
    ProcessorLayout layout;
    if (auto configurationError =
            configureProcessor(*candidate, sampleRate, blockSize, mode, sidechainInput, layout))
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
    updateParameterCount(*candidate);
    const auto candidateParameterCount =
        static_cast<std::size_t>(candidate->getParameters().size());
    pendingMidi.reset();
    const auto inputChannels = candidate->getMainBusNumInputChannels();
    const auto outputChannels = candidate->getMainBusNumOutputChannels();
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    juce::String parameterQueueError;
    if (!allocateParameterQueue(candidateParameterCount, parameterQueueError))
        return PluginLoadError{"parameterQueue", parameterQueueError};
    if (plugin != nullptr) plugin->releaseResources();
    plugin = std::move(candidate);
    staticState.reset();
    staticStateDirty.store(true, std::memory_order_release);
    automationActive.store(false, std::memory_order_release);
    adoptProcessorLayout(layout, blockSize);
    controlBuffer.setSize(processChannelCount, blockSize);
    preparedLatencySamples = std::max(0, plugin->getLatencySamples());
    const auto tailSeconds = plugin->getTailLengthSeconds();
    preparedTailSamples =
        std::isfinite(tailSeconds) && tailSeconds > 0.0
            ? static_cast<int>(std::min(sampleRate * 30.0, std::ceil(tailSeconds * sampleRate)))
            : 0;
    {
        const juce::ScopedLock statusGuard(statusLock);
        pluginPath = path;
        pluginName = description->name;
    }
    preparedSampleRate.store(sampleRate, std::memory_order_release);
    preparedBlockSize.store(blockSize, std::memory_order_release);
    pluginInputChannels.store(inputChannels, std::memory_order_release);
    pluginOutputChannels.store(outputChannels, std::memory_order_release);
    pluginIsInstrument.store(description->isInstrument, std::memory_order_release);
    cachedProgramCount.store(candidateProgramCount, std::memory_order_release);
    cachedHasEditor.store(candidateHasEditor, std::memory_order_release);
    bypassed.store(false, std::memory_order_release);
    panicPending.store(true, std::memory_order_release);
    bypassedBlocks.store(0, std::memory_order_release);
    processedBlocks.store(0, std::memory_order_release);
    transitionBlocks.store(0, std::memory_order_release);
    loaded.store(true, std::memory_order_release);
    loadCount.fetch_add(1, std::memory_order_relaxed);
    return std::nullopt;
}

PluginRack::~PluginRack() {
    if (plugin == nullptr) return;
    try {
        FaultInjection::before(FaultStage::destroy);
        plugin->releaseResources();
    } catch (...) {
    }
}

std::optional<PluginLoadError> PluginRack::configureProcessor(
    juce::AudioProcessor& processor, const double sampleRate, const int blockSize,
    const PluginProcessingMode mode, const bool sidechainInput, ProcessorLayout& layout) {
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
    // The auxiliary input is the first input bus published after the main input bus.
    const auto auxBusIndex = processor.getBusCount(true) >= 2 ? 1 : -1;
    const auto acceptsLayout = [&](const int mainChannels, const int auxChannels) {
        return processor.checkBusesLayoutSupported(layoutWithMainBuses(
            processor,
            mainChannels == 1 ? juce::AudioChannelSet::mono() : juce::AudioChannelSet::stereo(),
            juce::AudioChannelSet::stereo(), auxBusIndex, auxChannels));
    };
    layout.sidechain.available = auxBusIndex >= 0 && hasInputBus;
    layout.sidechain.mono =
        layout.sidechain.available && (acceptsLayout(2, 1) || acceptsLayout(1, 1));
    layout.sidechain.stereo =
        layout.sidechain.available && (acceptsLayout(2, 2) || acceptsLayout(1, 2));
    const auto enableSidechainInput =
        sidechainInput && (layout.sidechain.mono || layout.sidechain.stereo);
    if (!layout.sidechain.available) {
        layout.sidechain.reason = "The VST3 does not expose an auxiliary input bus.";
    } else if (!layout.sidechain.mono && !layout.sidechain.stereo) {
        layout.sidechain.reason = "The VST3 does not support a mono or stereo auxiliary input.";
    }

    std::optional<juce::AudioProcessor::BusesLayout> selected;
    if (enableSidechainInput) {
        if (layout.sidechain.available) {
            for (const auto mainChannels : {2, 1}) {
                for (const auto auxChannels : {2, 1}) {
                    auto candidate = layoutWithMainBuses(
                        processor,
                        mainChannels == 1 ? juce::AudioChannelSet::mono()
                                          : juce::AudioChannelSet::stereo(),
                        juce::AudioChannelSet::stereo(), auxBusIndex, auxChannels);
                    if (processor.checkBusesLayoutSupported(candidate)) {
                        selected = candidate;
                        break;
                    }
                }
                if (selected) break;
            }
        }
    } else {
        std::vector<juce::AudioProcessor::BusesLayout> candidates;
        if (hasInputBus) {
            candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::stereo(),
                                                     juce::AudioChannelSet::stereo(), -1, 0));
            candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::mono(),
                                                     juce::AudioChannelSet::stereo(), -1, 0));
        } else {
            candidates.push_back(layoutWithMainBuses(processor, juce::AudioChannelSet::disabled(),
                                                     juce::AudioChannelSet::stereo(), -1, 0));
        }
        const auto found =
            std::find_if(candidates.begin(), candidates.end(), [&processor](const auto& candidate) {
                return processor.checkBusesLayoutSupported(candidate);
            });
        if (found != candidates.end()) selected = *found;
    }
    if (!selected || !processor.setBusesLayout(*selected)) {
        return PluginLoadError{
            "pluginLayout",
            enableSidechainInput ? "The VST3 does not support a mono or stereo auxiliary input."
            : hasInputBus
                ? "The VST3 supports neither stereo-to-stereo nor mono-to-stereo processing."
                : "The VST3 does not support stereo output.",
        };
    }

    try {
        FaultInjection::before(FaultStage::prepare);
        processor.setNonRealtime(mode == PluginProcessingMode::offline);
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

    // Record where the enabled buses publish their channels in the process block buffer.
    if (auxBusIndex >= 0)
        for (int channel = 0; channel < processor.getChannelCountOfBus(true, auxBusIndex);
             ++channel)
            layout.auxInputOffsets.push_back(
                processor.getChannelIndexInProcessBlockBuffer(true, auxBusIndex, channel));
    layout.sidechain.channels = static_cast<int>(layout.auxInputOffsets.size());
    auto inputChannels = 0;
    for (int bus = 0; bus < processor.getBusCount(true); ++bus)
        inputChannels += processor.getChannelCountOfBus(true, bus);
    auto outputChannels = 0;
    for (int bus = 0; bus < processor.getBusCount(false); ++bus)
        outputChannels += processor.getChannelCountOfBus(false, bus);
    layout.processChannels = juce::jmax(inputChannels, outputChannels);
    return std::nullopt;
}

void PluginRack::adoptProcessorLayout(const ProcessorLayout& layout, const int blockSize) noexcept {
    auxInputOffsets = layout.auxInputOffsets;
    // The engine presents stereo output to every rack; a narrower plugin layout
    // keeps silent trailing channels in the process block buffer.
    processChannelCount = juce::jmax(layout.processChannels, 2);
    processPointers.assign(static_cast<std::size_t>(processChannelCount), nullptr);
    auxInputBuffer.setSize(static_cast<int>(layout.auxInputOffsets.size()), blockSize, false, true,
                           false);
    pluginAuxInputChannels.store(static_cast<int>(layout.auxInputOffsets.size()),
                                 std::memory_order_release);
    pluginSidechainAvailable.store(layout.sidechain.available, std::memory_order_release);
    pluginSidechainMono.store(layout.sidechain.mono, std::memory_order_release);
    pluginSidechainStereo.store(layout.sidechain.stereo, std::memory_order_release);
    const juce::ScopedLock guard(statusLock);
    sidechainUnavailableReason = layout.sidechain.reason;
}

void PluginRack::release() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->releaseResources();
}

void PluginRack::reset() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr) plugin->reset();
    panicPending.store(true, std::memory_order_release);
}

void PluginRack::setBypassed(const bool shouldBypass) noexcept {
    staticStateDirty.store(true, std::memory_order_release);
    bypassed.store(shouldBypass, std::memory_order_release);
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
           pluginIsInstrument.load(std::memory_order_acquire);
}

int PluginRack::latencySamples() const noexcept {
    return bypassed.load(std::memory_order_acquire)
               ? 0
               : preparedLatencySamples.load(std::memory_order_acquire);
}

int PluginRack::tailSamples() const noexcept { return preparedTailSamples; }

std::uint64_t PluginRack::droppedMidiEvents() const noexcept { return pendingMidi.droppedEvents(); }

bool PluginRack::processControlBlock() noexcept {
    if (plugin == nullptr || controlBuffer.getNumSamples() <= 0) return false;
    controlBuffer.clear();
    controlMidi.clear();
    // Do not consume live MIDI, change panic state, or send this block to the mixer.
    plugin->processBlock(controlBuffer, controlMidi);
    processedBlocks.fetch_add(1, std::memory_order_relaxed);
    completedControlSequence.store(requestedControlSequence.load(std::memory_order_acquire),
                                   std::memory_order_release);
    return true;
}

void PluginRack::process(const float* const* inputChannelData, const int numInputChannels,
                         const float* const* auxInputChannelData, const int numAuxInputChannels,
                         float* const* outputChannelData, const int numOutputChannels,
                         const int numSamples, const juce::MidiBuffer* const timelineMidi,
                         std::span<PluginParameterAutomation> automation,
                         const std::int64_t timelineSample, const bool playing,
                         const PluginTransportPosition* transport) noexcept {
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

    auto* active = plugin.get();
    if (active == nullptr || numOutputChannels <= 0 || numSamples <= 0) {
        return;
    }
    applyQueuedParameterChanges();
    applyAutomation(automation, timelineSample, playing);
    if (bypassed.load(std::memory_order_acquire)) {
        bypassedBlocks.fetch_add(1, std::memory_order_relaxed);
        return;
    }

    const auto requiredOutputs = pluginOutputChannels.load(std::memory_order_acquire);
    for (int channel = requiredOutputs; channel < numOutputChannels; ++channel)
        if (outputChannelData[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannelData[channel], numSamples);

    jassert(numOutputChannels <= processChannelCount);
    for (int channel = 0; channel < numOutputChannels; ++channel)
        processPointers[channel] = outputChannelData[channel];
    const auto auxChannels = static_cast<int>(auxInputOffsets.size());
    if (auxChannels > 0) {
        jassert(numSamples <= auxInputBuffer.getNumSamples());
        const auto* firstSource = numAuxInputChannels > 0 && auxInputChannelData != nullptr
                                      ? auxInputChannelData[0]
                                      : nullptr;
        const auto* secondSource = numAuxInputChannels > 1 && auxInputChannelData != nullptr
                                       ? auxInputChannelData[1]
                                       : nullptr;
        if (auxChannels == 1) {
            auto* target = auxInputBuffer.getWritePointer(0);
            if (firstSource == nullptr && secondSource == nullptr) {
                juce::FloatVectorOperations::clear(target, numSamples);
            } else if (firstSource == nullptr) {
                juce::FloatVectorOperations::copy(target, secondSource, numSamples);
            } else if (secondSource == nullptr) {
                juce::FloatVectorOperations::copy(target, firstSource, numSamples);
            } else {
                juce::FloatVectorOperations::add(target, firstSource, secondSource, numSamples);
                juce::FloatVectorOperations::multiply(target, 0.5f, numSamples);
            }
        } else {
            auto* firstTarget = auxInputBuffer.getWritePointer(0);
            auto* secondTarget = auxInputBuffer.getWritePointer(1);
            if (firstSource == nullptr && secondSource == nullptr) {
                juce::FloatVectorOperations::clear(firstTarget, numSamples);
                juce::FloatVectorOperations::clear(secondTarget, numSamples);
            } else if (firstSource == nullptr) {
                juce::FloatVectorOperations::copy(firstTarget, secondSource, numSamples);
                juce::FloatVectorOperations::copy(secondTarget, secondSource, numSamples);
            } else if (secondSource == nullptr) {
                juce::FloatVectorOperations::copy(firstTarget, firstSource, numSamples);
                juce::FloatVectorOperations::copy(secondTarget, firstSource, numSamples);
            } else {
                juce::FloatVectorOperations::copy(firstTarget, firstSource, numSamples);
                juce::FloatVectorOperations::copy(secondTarget, secondSource, numSamples);
            }
        }
        for (int channel = 0; channel < auxChannels; ++channel) {
            const auto offset = auxInputOffsets[channel];
            if (offset < numOutputChannels)
                juce::FloatVectorOperations::copy(
                    outputChannelData[offset], auxInputBuffer.getReadPointer(channel), numSamples);
            else
                processPointers[offset] = auxInputBuffer.getWritePointer(channel);
        }
    }

    juce::AudioBuffer<float> buffer(processPointers.data(), processChannelCount, numSamples);
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
    if (transport != nullptr) setTransportPosition(*transport);
    active->processBlock(buffer, processMidi);
    processedBlocks.fetch_add(1, std::memory_order_relaxed);
    completedControlSequence.store(requestedControlSequence.load(std::memory_order_acquire),
                                   std::memory_order_release);
}

void PluginRack::setTransportPosition(const PluginTransportPosition& transport) noexcept {
    auto& position = playHead.position;
    position.setTimeInSamples(transport.samplePosition);
    if (transport.sampleRate > 0.0)
        position.setTimeInSeconds(transport.samplePosition / transport.sampleRate);
    position.setPpqPosition(transport.ppqPosition);
    position.setBpm(transport.bpm);
    position.setPpqPositionOfLastBarStart(transport.lastBarStartPpq);
    position.setTimeSignature(
        juce::AudioPlayHead::TimeSignature{transport.numerator, transport.denominator});
    position.setIsPlaying(transport.playing);
    position.setIsRecording(transport.recording);
    position.setIsLooping(transport.looping);
    position.setLoopPoints(
        juce::AudioPlayHead::LoopPoints{transport.loopStartPpq, transport.loopEndPpq});
}

void PluginRack::notifyTransportStopped(const PluginTransportPosition& transport,
                                        const int numSamples) noexcept {
    jassert(!transport.playing);
    if (numSamples <= 0 || numSamples > controlBuffer.getNumSamples()) return;
    // The stopped block takes the regular processing path so pending panic, queued parameters,
    // and bypass apply to it; its output stays in the control buffer instead of the mix.
    process(nullptr, 0, nullptr, 0, controlBuffer.getArrayOfWritePointers(), 2, numSamples, nullptr,
            {}, transport.samplePosition, false, &transport);
}

}  // namespace riffra
