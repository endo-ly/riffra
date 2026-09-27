#include "OfflineRenderer.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <memory>
#include <utility>

#include "timeline/TimelineEngine.h"
#include "timeline/TimelineTimebase.h"

namespace riffra {

namespace {

std::unique_ptr<juce::AudioFormatWriter> createWriter(const juce::File& file,
                                                      const double sampleRate,
                                                      juce::String& error) {
    file.deleteFile();
    std::unique_ptr<juce::OutputStream> stream = file.createOutputStream();
    if (stream == nullptr) {
        error = "Offline Render output could not be opened.";
        return {};
    }
    juce::WavAudioFormat wav;
    const auto options =
        juce::AudioFormatWriterOptions{}
            .withSampleRate(sampleRate)
            .withNumChannels(2)
            .withBitsPerSample(32)
            .withSampleFormat(juce::AudioFormatWriterOptions::SampleFormat::floatingPoint);
    auto writer = wav.createWriterFor(stream, options);
    if (writer == nullptr) error = "Offline Render WAV writer could not be created.";
    return writer;
}

bool normalizeFile(const juce::File& source, const juce::File& destination,
                   juce::AudioFormatManager& formats, const float gain, juce::String& error) {
    auto sourceStream = source.createInputStream();
    if (sourceStream == nullptr || !sourceStream->openedOk()) {
        error = "Offline Render normalization source could not be reopened.";
        return false;
    }
    auto reader =
        std::unique_ptr<juce::AudioFormatReader>(formats.createReaderFor(std::move(sourceStream)));
    if (reader == nullptr) {
        error = "Offline Render normalization source could not be reopened.";
        return false;
    }
    auto writer = createWriter(destination, reader->sampleRate, error);
    if (writer == nullptr) return false;
    constexpr int blockSize = 4096;
    juce::AudioBuffer<float> buffer(2, blockSize);
    std::int64_t position = 0;
    while (position < reader->lengthInSamples) {
        const auto count =
            static_cast<int>(std::min<std::int64_t>(blockSize, reader->lengthInSamples - position));
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

}  // namespace

bool OfflineRenderer::render(const OfflineRenderRequestSpec& request,
                             juce::AudioFormatManager& formats, Result& result,
                             juce::String& error) {
    const auto sampleRate = static_cast<double>(request.sampleRate);
    const auto blockSize = static_cast<int>(request.blockSize);
    const auto destination = juce::File(request.destination);
    if (request.endTick <= request.startTick || sampleRate <= 0.0 || blockSize <= 0) {
        error = "Offline Render request is invalid.";
        return false;
    }
    const TimelineTimebase timelineTimebase{request.graph.timebase.ppq, request.graph.timebase.bpm};
    const auto startSample = timelineTimebase.tickToSample(request.startTick, sampleRate);
    const auto endSample = timelineTimebase.tickToSample(request.endTick, sampleRate);
    if (startSample < 0 || endSample <= startSample) {
        error = "Offline Render range has no samples.";
        return false;
    }
    if (!destination.getParentDirectory().createDirectory()) {
        error = "Offline Render output directory could not be created.";
        return false;
    }

    auto renderGraph = request.graph;
    renderGraph.metronomeEnabled = false;
    renderGraph.loopRange.enabled = false;
    const TimelineSnapshotSpec renderSnapshot{{}, 0, std::move(renderGraph)};

    TimelineEngine engine(true);
    if (!engine.loadSnapshot(renderSnapshot, formats, sampleRate, blockSize, error)) return false;
    engine.seekToTick(0);
    engine.play();

    const auto partial = destination.getSiblingFile(destination.getFileName() + ".partial");
    const auto normalized = destination.getSiblingFile(destination.getFileName() + ".normalized");
    partial.deleteFile();
    normalized.deleteFile();
    destination.deleteFile();
    auto writer = createWriter(partial, sampleRate, error);
    if (writer == nullptr) return false;

    juce::AudioBuffer<float> buffer(2, blockSize);
    std::int64_t position = 0;
    float peak = 0.0f;
    const auto masterGain =
        juce::Decibels::decibelsToGain(static_cast<float>(request.graph.masterGainDb));
    while (position < endSample) {
        const auto count =
            static_cast<int>(std::min<std::int64_t>(blockSize, endSample - position));
        buffer.clear();
        engine.mix(buffer.getArrayOfWritePointers(), 2, count);
        buffer.applyGain(0, count, masterGain);
        const auto writeStart = static_cast<int>(std::max<std::int64_t>(0, startSample - position));
        const auto writeCount = count - writeStart;
        if (writeCount > 0) {
            for (int channel = 0; channel < 2; ++channel)
                peak = std::max(peak, buffer.getMagnitude(channel, writeStart, writeCount));
            if (!writer->writeFromAudioSampleBuffer(buffer, writeStart, writeCount)) {
                error = "Offline Render WAV could not be written.";
                writer.reset();
                partial.deleteFile();
                return false;
            }
        }
        position += count;
    }
    writer.reset();

    const auto normalizationGain = request.normalize && peak > 0.0f ? 0.98f / peak : 1.0f;
    if (request.normalize && std::abs(normalizationGain - 1.0f) > 0.000001f) {
        if (!normalizeFile(partial, normalized, formats, normalizationGain, error) ||
            !normalized.moveFileTo(destination)) {
            partial.deleteFile();
            normalized.deleteFile();
            if (error.isEmpty()) error = "Offline Render normalized WAV could not be finalized.";
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

}  // namespace riffra
