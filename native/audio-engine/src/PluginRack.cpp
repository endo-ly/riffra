#include "PluginRack.h"

namespace riffra {

bool PluginRack::load(
    const juce::String& path,
    const double sampleRate,
    const int blockSize,
    juce::String& error) {
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
    pluginPath = path;
    pluginName = descriptions[0]->name;
    preparedSampleRate = sampleRate;
    preparedBlockSize = blockSize;
    plugin = std::move(candidate);
    bypassedBlocks.store(0, std::memory_order_release);
    return true;
}

void PluginRack::clear() noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    if (plugin != nullptr)
        plugin->releaseResources();
    plugin.reset();
    pluginPath.clear();
    pluginName.clear();
}

void PluginRack::prepare(const double sampleRate, const int blockSize) noexcept {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    preparedSampleRate = sampleRate;
    preparedBlockSize = blockSize;
    if (plugin != nullptr)
        plugin->prepareToPlay(sampleRate, blockSize);
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
        const auto* input = channel < numInputChannels ? inputChannelData[channel] : nullptr;
        if (input != nullptr)
            juce::FloatVectorOperations::copy(output, input, numSamples);
        else
            juce::FloatVectorOperations::clear(output, numSamples);
    }

    const juce::SpinLock::ScopedTryLockType lock(pluginLock);
    if (!lock.isLocked() || plugin == nullptr || numOutputChannels <= 0 || numSamples <= 0) {
        bypassedBlocks.fetch_add(1, std::memory_order_relaxed);
        return;
    }

    juce::AudioBuffer<float> buffer(outputChannelData, numOutputChannels, numSamples);
    juce::MidiBuffer midi;
    plugin->processBlock(buffer, midi);
}

juce::var PluginRack::status() const {
    const juce::SpinLock::ScopedLockType lock(pluginLock);
    auto* result = new juce::DynamicObject();
    result->setProperty("loaded", plugin != nullptr);
    result->setProperty("path", pluginPath);
    result->setProperty("name", pluginName);
    result->setProperty("sampleRate", preparedSampleRate);
    result->setProperty("blockSize", preparedBlockSize);
    result->setProperty(
        "bypassedBlocks",
        static_cast<juce::int64>(bypassedBlocks.load(std::memory_order_acquire)));
    return juce::var(result);
}

} // namespace riffra
