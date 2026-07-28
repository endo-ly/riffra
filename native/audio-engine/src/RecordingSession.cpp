#include "RecordingSession.h"

#include <algorithm>
#include <limits>
#include <utility>

namespace riffra {

namespace {
constexpr int kWriterBufferSamples = 131072;
constexpr int kBitsPerSample = 24;
}

std::unique_ptr<RecordingSession> RecordingSession::create(
    const juce::File& directory,
    const double sampleRate,
    const int rawChannels,
    const int processedChannels,
    juce::String& error) {
    if (sampleRate <= 0.0 || rawChannels <= 0 || processedChannels <= 0) {
        error = "Recording requires an active sample rate and at least one raw and processed channel.";
        return {};
    }
    auto session = std::unique_ptr<RecordingSession>(new RecordingSession(
        directory,
        sampleRate,
        rawChannels,
        processedChannels));
    if (!session->initialise(error))
        return {};
    return session;
}

RecordingSession::RecordingSession(
    juce::File directory,
    const double sampleRate,
    const int rawChannels,
    const int processedChannels)
    : recordingDirectory(std::move(directory)),
      rawPartial(recordingDirectory.getChildFile("raw.wav.partial")),
      processedPartial(recordingDirectory.getChildFile("processed.wav.partial")),
      rawFinal(recordingDirectory.getChildFile("raw.wav")),
      processedFinal(recordingDirectory.getChildFile("processed.wav")),
      manifest(recordingDirectory.getChildFile("manifest.json")),
      recordingSampleRate(sampleRate),
      rawChannelCount(rawChannels),
      processedChannelCount(processedChannels),
      startedAt(juce::Time::getCurrentTime()) {}

RecordingSession::~RecordingSession() {
    juce::String ignored;
    finish(ignored);
}

bool RecordingSession::initialise(juce::String& error) {
    if (recordingDirectory.exists() && !recordingDirectory.isDirectory()) {
        error = "Recording destination exists but is not a folder.";
        return false;
    }
    if (!recordingDirectory.createDirectory()) {
        error = "Recording destination could not be created.";
        return false;
    }
    if (rawPartial.existsAsFile() || processedPartial.existsAsFile()) {
        error = "Recording destination already contains an unfinished take.";
        return false;
    }

    writerThread.startThread(juce::Thread::Priority::normal);
    rawWriter = createWriter(
        rawPartial,
        recordingSampleRate,
        rawChannelCount,
        writerThread,
        error);
    if (rawWriter == nullptr)
        return false;
    processedWriter = createWriter(
        processedPartial,
        recordingSampleRate,
        processedChannelCount,
        writerThread,
        error);
    if (processedWriter == nullptr)
        return false;

    const auto flushSamples = juce::roundToInt(recordingSampleRate * 2.0);
    rawWriter->setFlushInterval(flushSamples);
    processedWriter->setFlushInterval(flushSamples);
    return writeManifest("recording", error);
}

std::unique_ptr<juce::AudioFormatWriter::ThreadedWriter> RecordingSession::createWriter(
    const juce::File& file,
    const double sampleRate,
    const int channels,
    juce::TimeSliceThread& thread,
    juce::String& error) {
    auto fileStream = file.createOutputStream();
    if (fileStream == nullptr || !fileStream->openedOk()) {
        error = "Recording file could not be opened: " + file.getFileName();
        return {};
    }
    std::unique_ptr<juce::OutputStream> stream = std::move(fileStream);
    juce::WavAudioFormat format;
    const auto options = juce::AudioFormatWriterOptions {}
        .withSampleRate(sampleRate)
        .withNumChannels(channels)
        .withBitsPerSample(kBitsPerSample);
    auto writer = format.createWriterFor(stream, options);
    if (writer == nullptr) {
        error = "WAV writer could not be created: " + file.getFileName();
        return {};
    }
    return std::make_unique<juce::AudioFormatWriter::ThreadedWriter>(
        writer.release(),
        thread,
        kWriterBufferSamples);
}

bool RecordingSession::write(
    const float* const* rawData,
    const float* const* processedData,
    const int numSamples) noexcept {
    if (finished || rawWriter == nullptr || processedWriter == nullptr || numSamples <= 0)
        return false;
    const auto rawWritten = writeRaw(rawData, numSamples);
    const auto processedWritten = writeProcessed(processedData, numSamples);
    return rawWritten && processedWritten;
}

bool RecordingSession::writeRaw(const float* const* rawData, const int numSamples) noexcept {
    if (finished || rawWriter == nullptr || numSamples <= 0)
        return false;
    const auto attemptedStart = rawAttemptedSamples.fetch_add(
        static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
    if (!rawWriter->write(rawData, numSamples)) {
        rawDroppedBlocks.fetch_add(1, std::memory_order_relaxed);
        rawMissingSamples.fetch_add(static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
        auto expected = std::numeric_limits<std::uint64_t>::max();
        rawFirstMissingSample.compare_exchange_strong(expected, attemptedStart, std::memory_order_relaxed);
        rawLastMissingSample.store(
            attemptedStart + static_cast<std::uint64_t>(numSamples),
            std::memory_order_relaxed);
        return false;
    }
    rawSamplesWritten.fetch_add(static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
    samplesWritten.fetch_add(static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
    return true;
}

bool RecordingSession::writeProcessed(
    const float* const* processedData,
    const int numSamples) noexcept {
    if (finished || processedWriter == nullptr || numSamples <= 0)
        return false;
    const auto attemptedStart = processedAttemptedSamples.fetch_add(
        static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
    if (!processedWriter->write(processedData, numSamples)) {
        processedDroppedBlocks.fetch_add(1, std::memory_order_relaxed);
        processedMissingSamples.fetch_add(
            static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
        auto expected = std::numeric_limits<std::uint64_t>::max();
        processedFirstMissingSample.compare_exchange_strong(
            expected, attemptedStart, std::memory_order_relaxed);
        processedLastMissingSample.store(
            attemptedStart + static_cast<std::uint64_t>(numSamples),
            std::memory_order_relaxed);
        return false;
    }
    processedSamplesWritten.fetch_add(
        static_cast<std::uint64_t>(numSamples), std::memory_order_relaxed);
    return true;
}

juce::File RecordingSession::flushRaw() noexcept {
    rawWriter.reset();
    if (rawPartial.existsAsFile())
        return rawPartial;
    return {};
}

bool RecordingSession::finish(juce::String& error) {
    if (finished)
        return true;
    finished = true;
    rawWriter.reset();
    processedWriter.reset();
    writerThread.stopThread(5000);

    auto completed = getRawSamplesWritten() > 0 && getProcessedSamplesWritten() > 0;
    if (!completed)
        error << "Recording contains no audio samples; empty partial files were kept for diagnosis. ";
    if (!rawPartial.existsAsFile()) {
        completed = false;
        error << "Raw recording file is missing. ";
    } else if (completed && !rawPartial.moveFileTo(rawFinal)) {
        completed = false;
        error << "Raw recording remains recoverable at " << rawPartial.getFullPathName() << ". ";
    }
    if (!processedPartial.existsAsFile()) {
        completed = false;
        error << "Processed recording file is missing. ";
    } else if (completed && !processedPartial.moveFileTo(processedFinal)) {
        completed = false;
        error << "Processed recording remains recoverable at "
              << processedPartial.getFullPathName() << ".";
    }
    juce::String manifestError;
    if (!writeManifest(completed ? "completed" : "recoverable", manifestError)) {
        completed = false;
        error << " Manifest update failed: " << manifestError;
    }
    return completed;
}

std::uint64_t RecordingSession::getSamplesWritten() const noexcept {
    return std::max(getRawSamplesWritten(), getProcessedSamplesWritten());
}

std::uint64_t RecordingSession::getRawSamplesWritten() const noexcept {
    return rawSamplesWritten.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getProcessedSamplesWritten() const noexcept {
    return processedSamplesWritten.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getRawAttemptedSamples() const noexcept {
    return rawAttemptedSamples.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getProcessedAttemptedSamples() const noexcept {
    return processedAttemptedSamples.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getRawDroppedBlocks() const noexcept {
    return rawDroppedBlocks.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getProcessedDroppedBlocks() const noexcept {
    return processedDroppedBlocks.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getRawMissingSamples() const noexcept {
    return rawMissingSamples.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getProcessedMissingSamples() const noexcept {
    return processedMissingSamples.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getRawFirstMissingSample() const noexcept {
    const auto value = rawFirstMissingSample.load(std::memory_order_acquire);
    return value == std::numeric_limits<std::uint64_t>::max() ? 0 : value;
}

std::uint64_t RecordingSession::getRawLastMissingSample() const noexcept {
    return rawLastMissingSample.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getProcessedFirstMissingSample() const noexcept {
    const auto value = processedFirstMissingSample.load(std::memory_order_acquire);
    return value == std::numeric_limits<std::uint64_t>::max() ? 0 : value;
}

std::uint64_t RecordingSession::getProcessedLastMissingSample() const noexcept {
    return processedLastMissingSample.load(std::memory_order_acquire);
}

std::uint64_t RecordingSession::getDroppedBlocks() const noexcept {
    return getRawDroppedBlocks() + getProcessedDroppedBlocks();
}

std::uint64_t RecordingSession::getMissingSamples() const noexcept {
    return getRawMissingSamples() + getProcessedMissingSamples();
}

std::uint64_t RecordingSession::getFirstMissingSample() const noexcept {
    const auto raw = getRawFirstMissingSample();
    const auto processed = getProcessedFirstMissingSample();
    if (raw == 0) return processed;
    if (processed == 0) return raw;
    return std::min(raw, processed);
}

std::uint64_t RecordingSession::getLastMissingSample() const noexcept {
    return std::max(
        getRawLastMissingSample(), getProcessedLastMissingSample());
}

juce::var RecordingSession::status() const {
    auto* result = new juce::DynamicObject();
    result->setProperty("active", !finished);
    result->setProperty("directory", recordingDirectory.getFullPathName());
    result->setProperty("sampleRate", recordingSampleRate);
    result->setProperty("rawChannels", rawChannelCount);
    result->setProperty("processedChannels", processedChannelCount);
    result->setProperty("samplesWritten", static_cast<juce::int64>(getSamplesWritten()));
    result->setProperty("droppedBlocks", static_cast<juce::int64>(getDroppedBlocks()));
    result->setProperty("missingSamples", static_cast<juce::int64>(getMissingSamples()));
    result->setProperty("dropoutStartSample", static_cast<juce::int64>(getFirstMissingSample()));
    result->setProperty("dropoutEndSample", static_cast<juce::int64>(getLastMissingSample()));
    result->setProperty("rawAttemptedSamples", static_cast<juce::int64>(getRawAttemptedSamples()));
    result->setProperty(
        "processedAttemptedSamples",
        static_cast<juce::int64>(getProcessedAttemptedSamples()));
    result->setProperty("rawDroppedBlocks", static_cast<juce::int64>(getRawDroppedBlocks()));
    result->setProperty(
        "processedDroppedBlocks",
        static_cast<juce::int64>(getProcessedDroppedBlocks()));
    result->setProperty("rawMissingSamples", static_cast<juce::int64>(getRawMissingSamples()));
    result->setProperty(
        "processedMissingSamples",
        static_cast<juce::int64>(getProcessedMissingSamples()));
    result->setProperty(
        "rawDropoutStartSample",
        static_cast<juce::int64>(getRawFirstMissingSample()));
    result->setProperty(
        "rawDropoutEndSample",
        static_cast<juce::int64>(getRawLastMissingSample()));
    result->setProperty(
        "processedDropoutStartSample",
        static_cast<juce::int64>(getProcessedFirstMissingSample()));
    result->setProperty(
        "processedDropoutEndSample",
        static_cast<juce::int64>(getProcessedLastMissingSample()));
    result->setProperty("recoveryStatus", getDroppedBlocks() == 0 ? "clean" : "partial");
    return juce::var(result);
}

bool RecordingSession::writeManifest(const juce::String& state, juce::String& error) const {
    auto* object = new juce::DynamicObject();
    object->setProperty("state", state);
    object->setProperty("startedAt", startedAt.toISO8601(true));
    object->setProperty("updatedAt", juce::Time::getCurrentTime().toISO8601(true));
    object->setProperty("sampleRate", recordingSampleRate);
    object->setProperty("rawChannels", rawChannelCount);
    object->setProperty("processedChannels", processedChannelCount);
    object->setProperty("samplesWritten", static_cast<juce::int64>(getSamplesWritten()));
    object->setProperty("droppedBlocks", static_cast<juce::int64>(getDroppedBlocks()));
    object->setProperty("missingSamples", static_cast<juce::int64>(getMissingSamples()));
    object->setProperty("dropoutStartSample", static_cast<juce::int64>(getFirstMissingSample()));
    object->setProperty("dropoutEndSample", static_cast<juce::int64>(getLastMissingSample()));
    object->setProperty("rawAttemptedSamples", static_cast<juce::int64>(getRawAttemptedSamples()));
    object->setProperty(
        "processedAttemptedSamples",
        static_cast<juce::int64>(getProcessedAttemptedSamples()));
    object->setProperty("rawDroppedBlocks", static_cast<juce::int64>(getRawDroppedBlocks()));
    object->setProperty(
        "processedDroppedBlocks",
        static_cast<juce::int64>(getProcessedDroppedBlocks()));
    object->setProperty("rawMissingSamples", static_cast<juce::int64>(getRawMissingSamples()));
    object->setProperty(
        "processedMissingSamples",
        static_cast<juce::int64>(getProcessedMissingSamples()));
    object->setProperty(
        "rawDropoutStartSample",
        static_cast<juce::int64>(getRawFirstMissingSample()));
    object->setProperty(
        "rawDropoutEndSample",
        static_cast<juce::int64>(getRawLastMissingSample()));
    object->setProperty(
        "processedDropoutStartSample",
        static_cast<juce::int64>(getProcessedFirstMissingSample()));
    object->setProperty(
        "processedDropoutEndSample",
        static_cast<juce::int64>(getProcessedLastMissingSample()));
    object->setProperty("recoveryStatus", getDroppedBlocks() == 0 ? "clean" : "partial");
    object->setProperty("rawFile", rawFinal.existsAsFile() ? "raw.wav" : "raw.wav.partial");
    object->setProperty(
        "processedFile",
        processedFinal.existsAsFile() ? "processed.wav" : "processed.wav.partial");
    if (!manifest.replaceWithText(juce::JSON::toString(juce::var(object), true))) {
        error = "Recording manifest could not be written.";
        return false;
    }
    return true;
}

} // namespace riffra
