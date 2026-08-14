#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <fstream>
#include <memory>
#include <utility>
#include <vector>

#include "ArrangementCaptureSink.h"

namespace riffra {
namespace {

juce::String pluginTopologySignature(const juce::var& values) {
    juce::Array<juce::var> topology;
    const auto append = [&topology](const juce::var& value) {
        if (!value.isObject()) return;
        auto* device = new juce::DynamicObject();
        device->setProperty("id", value.getProperty("id", {}));
        device->setProperty("kind", value.getProperty("kind", {}));
        device->setProperty("path", value.getProperty("path", {}));
        device->setProperty("disabledPlaceholder", value.getProperty("disabledPlaceholder", false));
        topology.add(juce::var(device));
    };
    if (values.isArray()) {
        for (const auto& value : *values.getArray()) append(value);
    } else if (values.isObject()) {
        append(values);
    }
    return juce::JSON::toString(juce::var(topology), false);
}

bool writePcmWave(const juce::File& file, const std::uint32_t sampleRate,
                  const std::uint16_t channels, const std::uint32_t frames,
                  const std::int16_t sample) {
    std::ofstream stream(file.getFullPathName().toStdString(), std::ios::binary | std::ios::trunc);
    if (!stream) return false;
    const auto dataBytes = frames * channels * static_cast<std::uint32_t>(sizeof(std::int16_t));
    const auto byteRate = sampleRate * channels * static_cast<std::uint32_t>(sizeof(std::int16_t));
    const auto blockAlign = static_cast<std::uint16_t>(channels * sizeof(std::int16_t));
    const auto writeU16 = [&stream](const std::uint16_t value) {
        stream.write(reinterpret_cast<const char*>(&value), sizeof(value));
    };
    const auto writeU32 = [&stream](const std::uint32_t value) {
        stream.write(reinterpret_cast<const char*>(&value), sizeof(value));
    };
    stream.write("RIFF", 4);
    writeU32(36 + dataBytes);
    stream.write("WAVEfmt ", 8);
    writeU32(16);
    writeU16(1);
    writeU16(channels);
    writeU32(sampleRate);
    writeU32(byteRate);
    writeU16(blockAlign);
    writeU16(16);
    stream.write("data", 4);
    writeU32(dataBytes);
    for (std::uint64_t index = 0; index < static_cast<std::uint64_t>(frames) * channels; ++index)
        stream.write(reinterpret_cast<const char*>(&sample), sizeof(sample));
    return stream.good();
}

class CaptureIsolationSink final : public ArrangementCaptureSink {
public:
    explicit CaptureIsolationSink(juce::File dir = {}) : testDirectory(std::move(dir)) {}

    bool beginAudioTrackCapture(const juce::String& trackId,
                                const std::uint64_t audioClockStartSample,
                                const std::uint64_t timelineStartSample) noexcept override {
        receivedTrack = trackId;
        if (beginCount < static_cast<int>(beginAudioSamples.size())) {
            beginAudioSamples[static_cast<std::size_t>(beginCount)] = audioClockStartSample;
            beginTimelineSamples[static_cast<std::size_t>(beginCount)] = timelineStartSample;
            segmentRawSamples[static_cast<std::size_t>(beginCount)] = 0;
        }
        ++beginCount;
        currentRawSamples = 0;
        segmentStartSample = rawBuffer.size();
        return true;
    }
    void writeAudioTrack(const juce::String& trackId, const float* raw, const int rawSampleCount,
                         const float* const* processed,
                         const int processedSampleCount) noexcept override {
        receivedTrack = trackId;
        receivedSamples = rawSampleCount;
        currentRawSamples += std::max(0, rawSampleCount);
        totalRawSamples += std::max(0, rawSampleCount);
        totalProcessedSamples += std::max(0, processedSampleCount);
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
        isolated = raw != nullptr && processed != nullptr && processed[0] != nullptr &&
                   processed[1] != nullptr && rawSampleCount == processedSampleCount;
        for (int sample = 0; isolated && sample < rawSampleCount; ++sample)
            isolated = std::abs(raw[sample] - 0.05f) < 0.0001f &&
                       std::abs(processed[0][sample] - 0.05f) < 0.0001f &&
                       std::abs(processed[1][sample] - 0.05f) < 0.0001f;
    }
    bool writeProcessedAudioTrackOffline(const juce::String&, const float* const* processed,
                                         const int sampleCount, int) noexcept override {
        if (processed != nullptr && sampleCount > 0 && processed[0] != nullptr &&
            processed[1] != nullptr) {
            totalProcessedSamples += sampleCount;
            maxOfflineProcessedWriteSize = std::max(maxOfflineProcessedWriteSize, sampleCount);
            ++offlineProcessedWriteCalls;
        }
        return true;
    }

    bool endAudioTrackCapture(const juce::String&, const std::uint64_t audioClockEndSample,
                              const std::uint64_t timelineEndSample) noexcept override {
        if (endCount < static_cast<int>(endAudioSamples.size())) {
            endAudioSamples[static_cast<std::size_t>(endCount)] = audioClockEndSample;
            endTimelineSamples[static_cast<std::size_t>(endCount)] = timelineEndSample;
            segmentRawSamples[static_cast<std::size_t>(endCount)] = currentRawSamples;
        }
        segmentRanges.emplace_back(segmentStartSample, rawBuffer.size());
        ++endCount;
        return true;
    }
    bool completeAudioTrackTail(const juce::String&) noexcept override { return true; }

    void markLoopBoundary(const std::uint64_t audioClockSample) noexcept override {
        if (loopBoundaryCount < static_cast<int>(loopBoundarySamples.size()))
            loopBoundarySamples[static_cast<std::size_t>(loopBoundaryCount)] = audioClockSample;
        ++loopBoundaryCount;
    }
    void writeMidiTrack(const juce::String&, const juce::String&, const juce::MidiMessage&,
                        std::uint64_t) noexcept override {}
    void setCaptureRange(std::uint64_t, std::uint64_t, std::uint64_t,
                         std::uint64_t) noexcept override {}

    juce::File prepareRawForReading(const juce::String&) noexcept override {
        if (testDirectory == juce::File{} || rawBuffer.empty()) return {};
        const auto file = testDirectory.getChildFile("capture-isolation-raw.wav");
        file.deleteFile();
        std::unique_ptr<juce::OutputStream> os(file.createOutputStream());
        if (os == nullptr) return {};
        juce::WavAudioFormat wav;
        auto writer = wav.createWriterFor(
            os, juce::AudioFormatWriterOptions()
                    .withSampleRate(48000.0)
                    .withNumChannels(1)
                    .withBitsPerSample(32)
                    .withSampleFormat(juce::AudioFormatWriterOptions::SampleFormat::floatingPoint));
        if (writer == nullptr) return {};
        const auto numSamples = static_cast<int>(rawBuffer.size());
        juce::AudioBuffer<float> writeBuffer(1, numSamples);
        writeBuffer.copyFrom(0, 0, rawBuffer.data(), numSamples);
        writer->writeFromAudioSampleBuffer(writeBuffer, 0, numSamples);
        writer->flush();
        return file;
    }

    std::vector<std::pair<std::uint64_t, std::uint64_t>> getRawSegmentRanges(
        const juce::String&) noexcept override {
        return segmentRanges;
    }

    juce::String receivedTrack;
    int receivedSamples = 0;
    bool isolated = false;
    int beginCount = 0;
    int endCount = 0;
    int loopBoundaryCount = 0;
    int currentRawSamples = 0;
    int totalRawSamples = 0;
    int totalProcessedSamples = 0;
    int maxOfflineProcessedWriteSize = 0;
    int offlineProcessedWriteCalls = 0;
    std::array<std::uint64_t, 8> beginAudioSamples{};
    std::array<std::uint64_t, 8> beginTimelineSamples{};
    std::array<std::uint64_t, 8> endAudioSamples{};
    std::array<std::uint64_t, 8> endTimelineSamples{};
    std::array<int, 8> segmentRawSamples{};
    std::array<std::uint64_t, 8> loopBoundarySamples{};

private:
    juce::File testDirectory;
    std::vector<float> rawBuffer;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> segmentRanges;
    std::uint64_t segmentStartSample = 0;
};

class LoopDataCaptureSink final : public ArrangementCaptureSink {
public:
    explicit LoopDataCaptureSink(juce::File dir, juce::String name = "loop-data")
        : testDirectory(std::move(dir)), fileName(std::move(name)) {}

    bool beginAudioTrackCapture(const juce::String&, std::uint64_t,
                                std::uint64_t) noexcept override {
        ++segmentCount;
        segmentStartSample = rawBuffer.size();
        return true;
    }
    void writeAudioTrack(const juce::String&, const float* raw, int rawSampleCount,
                         const float* const* processed,
                         int processedSampleCount) noexcept override {
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
        if (processed != nullptr && processedSampleCount > 0 && processed[0] != nullptr &&
            processed[1] != nullptr) {
            processedLeft.insert(processedLeft.end(), processed[0],
                                 processed[0] + processedSampleCount);
            processedRight.insert(processedRight.end(), processed[1],
                                  processed[1] + processedSampleCount);
            maxProcessedWriteSize = std::max(maxProcessedWriteSize, processedSampleCount);
        }
        totalRaw += std::max(0, rawSampleCount);
        totalProcessed += std::max(0, processedSampleCount);
    }
    bool writeProcessedAudioTrackOffline(const juce::String&, const float* const* processed,
                                         const int sampleCount, int) noexcept override {
        if (processed != nullptr && sampleCount > 0 && processed[0] != nullptr &&
            processed[1] != nullptr) {
            processedLeft.insert(processedLeft.end(), processed[0], processed[0] + sampleCount);
            processedRight.insert(processedRight.end(), processed[1], processed[1] + sampleCount);
            maxOfflineProcessedWriteSize = std::max(maxOfflineProcessedWriteSize, sampleCount);
            ++offlineProcessedWriteCalls;
        }
        totalProcessed += std::max(0, sampleCount);
        return true;
    }
    bool endAudioTrackCapture(const juce::String&, std::uint64_t, std::uint64_t) noexcept override {
        segmentRanges.emplace_back(segmentStartSample, rawBuffer.size());
        return true;
    }
    bool completeAudioTrackTail(const juce::String&) noexcept override { return true; }
    void markLoopBoundary(std::uint64_t) noexcept override { ++boundaryCount; }
    void writeMidiTrack(const juce::String&, const juce::String&, const juce::MidiMessage&,
                        std::uint64_t) noexcept override {}
    void setCaptureRange(std::uint64_t, std::uint64_t, std::uint64_t,
                         std::uint64_t) noexcept override {}

    juce::File prepareRawForReading(const juce::String&) noexcept override {
        if (rawBuffer.empty()) return {};
        const auto file = testDirectory.getChildFile(fileName + "-raw.wav");
        file.deleteFile();
        std::unique_ptr<juce::OutputStream> os(file.createOutputStream());
        if (os == nullptr) return {};
        juce::WavAudioFormat wav;
        auto writer = wav.createWriterFor(
            os, juce::AudioFormatWriterOptions()
                    .withSampleRate(48000.0)
                    .withNumChannels(2)
                    .withBitsPerSample(32)
                    .withSampleFormat(juce::AudioFormatWriterOptions::SampleFormat::floatingPoint));
        if (writer == nullptr) return {};
        const auto numSamples = static_cast<int>(rawBuffer.size());
        juce::AudioBuffer<float> writeBuffer(2, numSamples);
        writeBuffer.copyFrom(0, 0, rawBuffer.data(), numSamples);
        writeBuffer.copyFrom(1, 0, rawBuffer.data(), numSamples);
        writer->writeFromAudioSampleBuffer(writeBuffer, 0, numSamples);
        writer->flush();
        return file;
    }

    std::vector<std::pair<std::uint64_t, std::uint64_t>> getRawSegmentRanges(
        const juce::String&) noexcept override {
        return segmentRanges;
    }

    std::vector<float> rawBuffer;
    std::vector<float> processedLeft;
    std::vector<float> processedRight;
    int totalRaw = 0;
    int totalProcessed = 0;
    int segmentCount = 0;
    int boundaryCount = 0;
    int maxProcessedWriteSize = 0;
    int maxOfflineProcessedWriteSize = 0;
    int offlineProcessedWriteCalls = 0;

private:
    juce::File testDirectory;
    juce::String fileName;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> segmentRanges;
    std::uint64_t segmentStartSample = 0;
};

}  // namespace
}  // namespace riffra
