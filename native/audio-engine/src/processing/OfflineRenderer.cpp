#include "OfflineRenderer.h"

#include "TimelineEngine.h"
#include "TimelineTimebase.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <memory>

namespace riffra {

namespace {

std::unique_ptr<juce::AudioFormatWriter> createWriter(
    const juce::File& file,
    const double sampleRate,
    juce::String& error) {
    file.deleteFile();
    std::unique_ptr<juce::OutputStream> stream = file.createOutputStream();
    if (stream == nullptr) {
        error = "Offline Render output could not be opened.";
        return {};
    }
    juce::WavAudioFormat wav;
    const auto options = juce::AudioFormatWriterOptions {}
                             .withSampleRate(sampleRate)
                             .withNumChannels(2)
                             .withBitsPerSample(32)
                             .withSampleFormat(
                                 juce::AudioFormatWriterOptions::SampleFormat::floatingPoint);
    auto writer = wav.createWriterFor(stream, options);
    if (writer == nullptr)
        error = "Offline Render WAV writer could not be created.";
    return writer;
}

bool normalizeFile(
    const juce::File& source,
    const juce::File& destination,
    juce::AudioFormatManager& formats,
    const float gain,
    juce::String& error) {
    auto reader = std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(source));
    if (reader == nullptr) {
        error = "Offline Render normalization source could not be reopened.";
        return false;
    }
    auto writer = createWriter(destination, reader->sampleRate, error);
    if (writer == nullptr)
        return false;
    constexpr int blockSize = 4096;
    juce::AudioBuffer<float> buffer(2, blockSize);
    std::int64_t position = 0;
    while (position < reader->lengthInSamples) {
        const auto count = static_cast<int>(std::min<std::int64_t>(
            blockSize, reader->lengthInSamples - position));
        buffer.clear();
        if (!reader->read(&buffer, 0, count, position, true, true)) {
            error = "Offline Render normalization source could not be read.";
            return false;
        }
        buffer.applyGain(0, count, gain);
        if (!writer->writeFromAudioSampleBuffer(buffer, 0, count)) {
            error = "Offline Render normalized WAV could not be written.";
            return false;
        }
        position += count;
    }
    writer.reset();
    return true;
}

} // namespace

bool OfflineRenderer::render(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const juce::File& destination,
    const std::uint64_t startTick,
    const std::uint64_t endTick,
    const double sampleRate,
    const int blockSize,
    const float masterGainDb,
    const bool normalize,
    Result& result,
    juce::String& error) {
    if (!snapshot.isObject() || endTick <= startTick || sampleRate <= 0.0
        || blockSize <= 0) {
        error = "Offline Render request is invalid.";
        return false;
    }
    const auto timebase = snapshot.getProperty("timebase", {});
    const auto ppq = static_cast<std::uint32_t>(
        static_cast<int>(timebase.getProperty("ppq", 0)));
    const auto bpm = static_cast<double>(timebase.getProperty("bpm", 0.0));
    const TimelineTimebase timelineTimebase { ppq, bpm };
    const auto startSample = timelineTimebase.tickToSample(startTick, sampleRate);
    const auto endSample = timelineTimebase.tickToSample(endTick, sampleRate);
    if (startSample < 0 || endSample <= startSample) {
        error = "Offline Render range has no samples.";
        return false;
    }
    if (!destination.getParentDirectory().createDirectory()) {
        error = "Offline Render output directory could not be created.";
        return false;
    }

    auto renderSnapshot = snapshot;
    if (auto* snapshotObject = renderSnapshot.getDynamicObject())
        snapshotObject->setProperty("metronomeEnabled", false);
    auto loopRange = renderSnapshot.getProperty("loopRange", {});
    if (auto* loopObject = loopRange.getDynamicObject())
        loopObject->setProperty("enabled", false);

    TimelineEngine engine(true);
    if (!engine.loadSnapshot(
            renderSnapshot, formats, sampleRate, blockSize, error))
        return false;
    engine.seekToTick(0);
    engine.play();

    const auto partial = destination.getSiblingFile(
        destination.getFileName() + ".partial");
    const auto normalized = destination.getSiblingFile(
        destination.getFileName() + ".normalized");
    partial.deleteFile();
    normalized.deleteFile();
    destination.deleteFile();
    auto writer = createWriter(partial, sampleRate, error);
    if (writer == nullptr)
        return false;

    juce::AudioBuffer<float> buffer(2, blockSize);
    std::int64_t position = 0;
    float peak = 0.0f;
    const auto masterGain = juce::Decibels::decibelsToGain(
        juce::jlimit(-90.0f, 0.0f, masterGainDb));
    while (position < endSample) {
        const auto count = static_cast<int>(std::min<std::int64_t>(
            blockSize, endSample - position));
        buffer.clear();
        engine.mix(buffer.getArrayOfWritePointers(), 2, count);
        buffer.applyGain(0, count, masterGain);
        const auto writeStart = static_cast<int>(std::max<std::int64_t>(
            0, startSample - position));
        const auto writeCount = count - writeStart;
        if (writeCount > 0) {
            for (int channel = 0; channel < 2; ++channel)
                peak = std::max(
                    peak,
                    buffer.getMagnitude(channel, writeStart, writeCount));
            if (!writer->writeFromAudioSampleBuffer(
                    buffer, writeStart, writeCount)) {
                error = "Offline Render WAV could not be written.";
                writer.reset();
                partial.deleteFile();
                return false;
            }
        }
        position += count;
    }
    writer.reset();

    const auto normalizationGain = normalize && peak > 0.0f
        ? 0.98f / peak
        : 1.0f;
    if (normalize && std::abs(normalizationGain - 1.0f) > 0.000001f) {
        if (!normalizeFile(partial, normalized, formats, normalizationGain, error)
            || !normalized.moveFileTo(destination)) {
            partial.deleteFile();
            normalized.deleteFile();
            if (error.isEmpty())
                error = "Offline Render normalized WAV could not be finalized.";
            return false;
        }
        partial.deleteFile();
    } else if (!partial.moveFileTo(destination)) {
        partial.deleteFile();
        error = "Offline Render WAV could not be finalized.";
        return false;
    }

    result.frames = static_cast<std::uint64_t>(endSample - startSample);
    result.sampleRate = sampleRate;
    return true;
}

} // namespace riffra
