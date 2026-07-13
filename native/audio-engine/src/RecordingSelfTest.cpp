#include "RecordingSelfTest.h"

#include "RecordingSession.h"

#include <array>
#include <cmath>
#include <memory>
#include <vector>

namespace riffra {

namespace {
juce::var result(bool ok, const juce::File& directory, const juce::String& message = {}) {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "recordingSelfTest");
    object->setProperty("ok", ok);
    object->setProperty("directory", directory.getFullPathName());
    if (message.isNotEmpty())
        object->setProperty("message", message);
    return juce::var(object);
}
} // namespace

juce::var runRecordingSelfTest(const juce::File& directory) {
    if (directory.exists())
        return result(false, directory, "Refusing to overwrite an existing self-test directory.");

    constexpr double sampleRate = 44100.0;
    constexpr int channels = 2;
    constexpr int blockSize = 128;
    constexpr int totalSamples = 22050;
    juce::String error;
    auto session = RecordingSession::create(directory, sampleRate, channels, channels, error);
    if (session == nullptr)
        return result(false, directory, error);

    std::array<std::vector<float>, channels> rawBuffers;
    std::array<std::vector<float>, channels> processedBuffers;
    for (auto& buffer : rawBuffers)
        buffer.resize(blockSize);
    for (auto& buffer : processedBuffers)
        buffer.resize(blockSize);
    std::array<const float*, channels> rawPointers {};
    std::array<const float*, channels> processedPointers {};

    for (int offset = 0; offset < totalSamples; offset += blockSize) {
        const auto count = juce::jmin(blockSize, totalSamples - offset);
        for (int sample = 0; sample < count; ++sample) {
            const auto phase = static_cast<double>(offset + sample) / sampleRate;
            const auto value = static_cast<float>(0.25 * std::sin(2.0 * juce::MathConstants<double>::pi * 440.0 * phase));
            rawBuffers[0][sample] = value;
            rawBuffers[1][sample] = -value;
            processedBuffers[0][sample] = value * 0.5f;
            processedBuffers[1][sample] = -value * 0.5f;
        }
        for (int channel = 0; channel < channels; ++channel) {
            rawPointers[static_cast<std::size_t>(channel)] = rawBuffers[static_cast<std::size_t>(channel)].data();
            processedPointers[static_cast<std::size_t>(channel)] = processedBuffers[static_cast<std::size_t>(channel)].data();
        }
        if (!session->write(rawPointers.data(), processedPointers.data(), count))
            return result(false, directory, "Synthetic audio block was dropped.");
    }

    if (!session->finish(error))
        return result(false, directory, error);

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    std::unique_ptr<juce::AudioFormatReader> rawReader(formats.createReaderFor(directory.getChildFile("raw.wav")));
    std::unique_ptr<juce::AudioFormatReader> processedReader(formats.createReaderFor(directory.getChildFile("processed.wav")));
    if (rawReader == nullptr || processedReader == nullptr)
        return result(false, directory, "Generated WAV files could not be reopened.");
    if (rawReader->numChannels != channels || processedReader->numChannels != channels
        || rawReader->lengthInSamples != totalSamples || processedReader->lengthInSamples != totalSamples)
        return result(false, directory, "Generated WAV metadata did not match the synthetic take.");

    const auto manifest = juce::JSON::parse(directory.getChildFile("manifest.json").loadFileAsString());
    if (!manifest.isObject() || manifest.getProperty("state", {}).toString() != "completed")
        return result(false, directory, "Recording manifest did not reach completed state.");
    if (static_cast<juce::int64>(manifest.getProperty("samplesWritten", {})) != totalSamples
        || manifest.getProperty("rawFile", {}).toString() != "raw.wav"
        || manifest.getProperty("processedFile", {}).toString() != "processed.wav")
        return result(false, directory, "Completed manifest did not describe the finalized WAV files.");

    const auto emptyDirectory = directory.getChildFile("empty-take");
    auto emptySession = RecordingSession::create(
        emptyDirectory,
        sampleRate,
        channels,
        channels,
        error);
    if (emptySession == nullptr)
        return result(false, directory, "Empty-take setup failed: " + error);
    error.clear();
    if (emptySession->finish(error))
        return result(false, directory, "An empty take was incorrectly finalized as completed.");
    const auto emptyManifest = juce::JSON::parse(
        emptyDirectory.getChildFile("manifest.json").loadFileAsString());
    if (!emptyManifest.isObject()
        || emptyManifest.getProperty("state", {}).toString() != "recoverable"
        || static_cast<juce::int64>(emptyManifest.getProperty("samplesWritten", {})) != 0
        || !emptyDirectory.getChildFile("raw.wav.partial").existsAsFile()
        || !emptyDirectory.getChildFile("processed.wav.partial").existsAsFile())
        return result(false, directory, "Empty take was not retained as recoverable partial data.");

    auto* object = new juce::DynamicObject();
    object->setProperty("type", "recordingSelfTest");
    object->setProperty("ok", true);
    object->setProperty("directory", directory.getFullPathName());
    object->setProperty("rawSamples", static_cast<juce::int64>(rawReader->lengthInSamples));
    object->setProperty("processedSamples", static_cast<juce::int64>(processedReader->lengthInSamples));
    object->setProperty("sampleRate", rawReader->sampleRate);
    object->setProperty("droppedBlocks", static_cast<juce::int64>(session->getDroppedBlocks()));
    return juce::var(object);
}

} // namespace riffra
