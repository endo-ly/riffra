#include "OfflineRenderer.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <memory>
#include <thread>
#include <utility>
#include <variant>

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

bool hostsPlugins(const ExecutionGraph& graph) noexcept {
    return std::any_of(graph.tracks.begin(), graph.tracks.end(), [](const auto& track) {
        return !track.effects.empty() ||
               (track.instrument.has_value() &&
                std::holds_alternative<Vst3InstrumentSpec>(*track.instrument));
    });
}

// Plugins such as disk-streaming samplers keep loading their content after instantiation and
// report no readiness to the host; they only become audible after being processed for a while
// with the message thread dispatching. The interval covers such loading with margin.
constexpr auto kPluginWarmUp = std::chrono::seconds(2);

void warmUpPlugins(TimelineEngine& engine, const double sampleRate, const int blockSize) {
    const auto blockDuration = std::chrono::duration_cast<std::chrono::steady_clock::duration>(
        std::chrono::duration<double>(blockSize / sampleRate));
    const auto deadline = std::chrono::steady_clock::now() + kPluginWarmUp;
    for (auto next = std::chrono::steady_clock::now(); next < deadline; next += blockDuration) {
        engine.warmUpDevices(blockSize);
        std::this_thread::sleep_until(next + blockDuration);
    }
}

}  // namespace

OfflineRenderer::OfflineRenderer(Plan renderPlan,
                                 std::unique_ptr<TimelineEngine> timelineEngine) noexcept
    : plan(std::move(renderPlan)), engine(std::move(timelineEngine)) {}

OfflineRenderer::~OfflineRenderer() = default;

std::unique_ptr<OfflineRenderer> OfflineRenderer::prepare(const OfflineRenderRequestSpec& request,
                                                          juce::AudioFormatManager& formats,
                                                          juce::String& error) {
    const auto sampleRate = static_cast<double>(request.sampleRate);
    const auto blockSize = static_cast<int>(request.blockSize);
    const auto destination = juce::File(request.destination);
    if (request.endTick <= request.startTick || sampleRate <= 0.0 || blockSize <= 0) {
        error = "Offline Render request is invalid.";
        return nullptr;
    }
    const TimelineTimebase timelineTimebase{request.graph.timebase.ppq, request.graph.timebase.bpm};
    const auto startSample = timelineTimebase.tickToSample(request.startTick, sampleRate);
    const auto endSample = timelineTimebase.tickToSample(request.endTick, sampleRate);
    if (startSample < 0 || endSample <= startSample) {
        error = "Offline Render range has no samples.";
        return nullptr;
    }
    if (!destination.getParentDirectory().createDirectory()) {
        error = "Offline Render output directory could not be created.";
        return nullptr;
    }

    auto renderGraph = request.graph;
    renderGraph.metronomeEnabled = false;
    renderGraph.loopRange.enabled = false;
    const TimelineSnapshotSpec renderSnapshot{{}, 0, std::move(renderGraph)};

    auto timelineEngine = std::make_unique<TimelineEngine>(true);
    if (!timelineEngine->loadSnapshot(renderSnapshot, formats, sampleRate, blockSize, error))
        return nullptr;
    Plan renderPlan{destination,
                    sampleRate,
                    blockSize,
                    startSample,
                    endSample,
                    juce::Decibels::decibelsToGain(static_cast<float>(request.graph.masterGainDb)),
                    request.normalize,
                    hostsPlugins(request.graph)};
    return std::unique_ptr<OfflineRenderer>(
        new OfflineRenderer(std::move(renderPlan), std::move(timelineEngine)));
}

bool OfflineRenderer::render(juce::AudioFormatManager& formats, Result& result,
                             juce::String& error) {
    if (plan.hostsPlugins) warmUpPlugins(*engine, plan.sampleRate, plan.blockSize);
    engine->seekToTick(0);
    engine->play();

    const auto& destination = plan.destination;
    const auto partial = destination.getSiblingFile(destination.getFileName() + ".partial");
    const auto normalized = destination.getSiblingFile(destination.getFileName() + ".normalized");
    partial.deleteFile();
    normalized.deleteFile();
    destination.deleteFile();
    auto writer = createWriter(partial, plan.sampleRate, error);
    if (writer == nullptr) return false;

    juce::AudioBuffer<float> buffer(2, plan.blockSize);
    std::int64_t position = 0;
    float peak = 0.0f;
    while (position < plan.endSample) {
        const auto count =
            static_cast<int>(std::min<std::int64_t>(plan.blockSize, plan.endSample - position));
        buffer.clear();
        engine->mix(buffer.getArrayOfWritePointers(), 2, count);
        buffer.applyGain(0, count, plan.masterGain);
        const auto writeStart =
            static_cast<int>(std::max<std::int64_t>(0, plan.startSample - position));
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

    const auto normalizationGain = plan.normalize && peak > 0.0f ? 0.98f / peak : 1.0f;
    if (plan.normalize && std::abs(normalizationGain - 1.0f) > 0.000001f) {
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

    result.frames = static_cast<std::uint64_t>(plan.endSample - plan.startSample);
    result.sampleRate = plan.sampleRate;
    return true;
}

}  // namespace riffra
