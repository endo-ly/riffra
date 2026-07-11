#pragma once

#include <JuceHeader.h>
#include <atomic>
#include <memory>

namespace riffra {

class RecordingSession final {
public:
    static std::unique_ptr<RecordingSession> create(
        const juce::File& directory,
        double sampleRate,
        int rawChannels,
        int processedChannels,
        juce::String& error);

    ~RecordingSession();

    RecordingSession(const RecordingSession&) = delete;
    RecordingSession& operator=(const RecordingSession&) = delete;

    bool write(
        const float* const* rawData,
        const float* const* processedData,
        int numSamples) noexcept;
    bool finish(juce::String& error);

    [[nodiscard]] int getRawChannels() const noexcept { return rawChannelCount; }
    [[nodiscard]] int getProcessedChannels() const noexcept { return processedChannelCount; }
    [[nodiscard]] std::uint64_t getSamplesWritten() const noexcept;
    [[nodiscard]] std::uint64_t getDroppedBlocks() const noexcept;
    [[nodiscard]] juce::File getDirectory() const { return recordingDirectory; }
    [[nodiscard]] juce::var status() const;

private:
    RecordingSession(
        juce::File directory,
        double sampleRate,
        int rawChannels,
        int processedChannels);

    bool initialise(juce::String& error);
    bool writeManifest(const juce::String& state, juce::String& error) const;
    static std::unique_ptr<juce::AudioFormatWriter::ThreadedWriter> createWriter(
        const juce::File& file,
        double sampleRate,
        int channels,
        juce::TimeSliceThread& thread,
        juce::String& error);

    juce::File recordingDirectory;
    juce::File rawPartial;
    juce::File processedPartial;
    juce::File rawFinal;
    juce::File processedFinal;
    juce::File manifest;
    double recordingSampleRate = 0.0;
    int rawChannelCount = 0;
    int processedChannelCount = 0;
    juce::Time startedAt;
    juce::TimeSliceThread writerThread { "Riffra recording writer" };
    std::unique_ptr<juce::AudioFormatWriter::ThreadedWriter> rawWriter;
    std::unique_ptr<juce::AudioFormatWriter::ThreadedWriter> processedWriter;
    std::atomic<std::uint64_t> samplesWritten { 0 };
    std::atomic<std::uint64_t> droppedBlocks { 0 };
    bool finished = false;
};

} // namespace riffra
