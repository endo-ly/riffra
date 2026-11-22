#include "OfflineRenderer.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <memory>
#include <thread>
#include <utility>
#include <variant>

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

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
    return !graph.mixer.masterEffects.empty() ||
           std::any_of(graph.mixer.buses.begin(), graph.mixer.buses.end(),
                       [](const auto& bus) { return !bus.effects.empty(); }) ||
           std::any_of(graph.tracks.begin(), graph.tracks.end(), [](const auto& track) {
               return !track.effects.empty() ||
                      (track.instrument.has_value() &&
                       std::holds_alternative<Vst3InstrumentSpec>(*track.instrument));
           });
}

}  // namespace

/// Waits in short slices for the paced loop. Windows' default scheduler tick (~15.6 ms)
/// stretches every slice, so the wait runs on a high-resolution timer; systems without one keep
/// the regular tick, which only makes the render slower than wall-clock time. A render owns one
/// waiter, so a timer is never shared with another render pacing in the same process.
class OfflineRenderer::PacingWaiter final {
public:
    PacingWaiter() noexcept {
#if JUCE_WINDOWS
        timer = CreateWaitableTimerExW(nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                                       TIMER_ALL_ACCESS);
#endif
    }
    ~PacingWaiter() {
#if JUCE_WINDOWS
        if (timer != nullptr) CloseHandle(timer);
#endif
    }

    PacingWaiter(const PacingWaiter&) = delete;
    PacingWaiter& operator=(const PacingWaiter&) = delete;

    void wait(const std::chrono::steady_clock::duration duration) const noexcept {
#if JUCE_WINDOWS
        LARGE_INTEGER due{};
        due.QuadPart = -std::max<std::int64_t>(
            1, std::chrono::duration_cast<std::chrono::nanoseconds>(duration).count() / 100);
        if (timer != nullptr && SetWaitableTimerEx(timer, &due, 0, nullptr, nullptr, nullptr, 0)) {
            WaitForSingleObject(timer, INFINITE);
            return;
        }
#endif
        std::this_thread::sleep_for(duration);
    }

private:
#if JUCE_WINDOWS
    HANDLE timer = nullptr;
#endif
};

OfflineRenderer::OfflineRenderer(Plan renderPlan,
                                 std::unique_ptr<TimelineEngine> timelineEngine) noexcept
    : plan(std::move(renderPlan)), engine(std::move(timelineEngine)) {}

OfflineRenderer::~OfflineRenderer() = default;

void OfflineRenderer::cancel() noexcept { cancelRequested.store(true, std::memory_order_release); }

bool OfflineRenderer::isCancelled() const noexcept {
    return cancelRequested.load(std::memory_order_acquire);
}

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
    if (!request.captures.empty() && (request.normalize || !request.stems.empty())) {
        error = "Capture requires an unnormalized capture-only output.";
        return nullptr;
    }
    TimelineTimebase timelineTimebase;
    timelineTimebase.ppq = request.graph.timebase.ppq;
    timelineTimebase.tempoChanges = request.graph.timebase.tempoChanges;
    timelineTimebase.timeSignatureChanges = request.graph.timebase.timeSignatureChanges;
    const auto startSample = timelineTimebase.tickToSample(request.startTick, sampleRate);
    const auto musicalEndSample = timelineTimebase.tickToSample(request.endTick, sampleRate);
    auto processFinalEvents = false;
    if (request.includeEndEvents) {
        for (const auto& track : request.graph.tracks) {
            for (const auto& clip : track.midiClips) {
                if (std::any_of(clip.instrumentControlEvents.begin(),
                                clip.instrumentControlEvents.end(),
                                [&clip, &request](const auto& control) {
                                    return clip.startTick + control.tick == request.endTick;
                                }))
                    processFinalEvents = true;
                if (std::any_of(clip.notes.begin(), clip.notes.end(),
                                [&clip, &request](const auto& note) {
                                    return clip.startTick + note.startTick + note.durationTicks ==
                                           request.endTick;
                                }))
                    processFinalEvents = true;
            }
        }
    }
    // The WAV write window is defined by tempo map, ticks, sample rate, and tail alone. Notes
    // and instrument controls ending exactly on the end tick need one extra processing sample
    // so their release applies before the write window closes, which must stay invisible to
    // the WAV length.
    const auto outputEndSample =
        musicalEndSample +
        static_cast<std::int64_t>(std::llround(request.tailSeconds * sampleRate));
    if (startSample < 0 || outputEndSample <= startSample) {
        error = "Offline Render range has no samples.";
        return nullptr;
    }
    if (request.stems.empty() && request.captures.empty() &&
        !destination.getParentDirectory().createDirectory()) {
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
    if (!request.stems.empty()) {
        std::vector<OfflineCaptureTarget> targets;
        for (const auto& stem : request.stems) {
            if (!juce::File(stem.destination).getParentDirectory().createDirectory()) {
                error = "Stem output directory could not be created: " + stem.destination;
                return nullptr;
            }
            targets.push_back({OfflineCaptureKind::track, stem.trackId});
        }
        if (!timelineEngine->prepareOutputCapture(targets, error)) return nullptr;
    }
    if (!request.captures.empty()) {
        std::vector<OfflineCaptureTarget> targets;
        for (const auto& capture : request.captures) {
            if (!juce::File(capture.destination).getParentDirectory().createDirectory()) {
                error = "Capture output directory could not be created: " + capture.destination;
                return nullptr;
            }
            targets.push_back(capture.target);
        }
        if (!timelineEngine->prepareOutputCapture(targets, error)) return nullptr;
    }
    const auto status = timelineEngine->status();
    const auto latencySamples = status.graph.has_value()
                                    ? static_cast<std::int64_t>(status.graph->maximumLatencySamples)
                                    : 0;
    // Latency compensation shifts each callback's audio later in the engine stream by the
    // graph maximum latency, so processing must continue until the stream passes the shifted
    // write window end. The shifted window already covers the extra processing sample for
    // notes and instrument controls ending exactly on the end tick whenever the graph
    // reports any latency.
    const auto processingEnd =
        std::max(outputEndSample + latencySamples,
                 processFinalEvents ? musicalEndSample + 1 : outputEndSample);
    const auto automationWarnings =
        status.graph ? status.graph->automationWarnings : std::vector<juce::String>{};
    Plan renderPlan{destination,
                    request.stems,
                    request.captures,
                    sampleRate,
                    blockSize,
                    startSample,
                    outputEndSample,
                    processingEnd,
                    static_cast<float>(request.graph.masterGainDb),
                    request.normalize,
                    hostsPlugins(request.graph),
                    request.graph.pluginWarmUpSeconds,
                    request.renderMode,
                    latencySamples,
                    automationWarnings};
    return std::unique_ptr<OfflineRenderer>(
        new OfflineRenderer(std::move(renderPlan), std::move(timelineEngine)));
}

bool OfflineRenderer::warmUpPlugins() {
    // Plugins such as disk-streaming samplers keep loading their content after instantiation
    // and report no readiness; they only become audible after being processed for a while with
    // the message thread dispatching. The configured interval covers such loading with margin.
    const auto blockDuration = std::chrono::duration_cast<std::chrono::steady_clock::duration>(
        std::chrono::duration<double>(plan.blockSize / plan.sampleRate));
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::duration<double>(plan.pluginWarmUpSeconds);
    for (auto next = std::chrono::steady_clock::now(); next < deadline && !isCancelled();
         next += blockDuration) {
        engine->warmUpPluginDevices(plan.blockSize);
        std::this_thread::sleep_until(next + blockDuration);
    }
    return !isCancelled();
}

bool OfflineRenderer::waitForPacing(const PacingWaiter& waiter,
                                    const std::chrono::steady_clock::time_point start,
                                    const std::int64_t processedSamples) {
    const auto target =
        start +
        std::chrono::duration_cast<std::chrono::steady_clock::duration>(
            std::chrono::duration<double>(static_cast<double>(processedSamples) / plan.sampleRate));
    constexpr auto slice = std::chrono::milliseconds(25);
    while (!isCancelled()) {
        const auto now = std::chrono::steady_clock::now();
        if (now >= target) return true;
        const auto remaining = target - now;
        waiter.wait(remaining < slice ? remaining : slice);
    }
    return false;
}

bool OfflineRenderer::render(juce::AudioFormatManager& formats, Result& result,
                             juce::String& error) {
    if (plan.hostsPlugins) {
        if (!warmUpPlugins()) {
            error = "Offline Render was cancelled.";
            return false;
        }
        // The warm-up advanced the plugins' processing state; rendering starts from reset state.
        if (!juce::MessageManager::callSync([this] { engine->resetPluginDevices(); })) {
            error = "Offline Render could not reset the warmed-up plugins.";
            return false;
        }
    }
    // The render thread owns the engine's realtime state, so both requests apply immediately.
    if (!engine->seekToTick(0).has_value() || !engine->play().has_value()) {
        error = "Offline Render transport could not start.";
        return false;
    }

    struct Output final {
        juce::File destination, partial, normalized;
        std::unique_ptr<juce::AudioFormatWriter> writer;
        float peak = 0.0f;
        bool published = false;
    };
    struct Outputs final {
        std::vector<Output> files;
        bool committed = false;
        ~Outputs() {
            for (auto& file : files) {
                file.writer.reset();
                file.partial.deleteFile();
                file.normalized.deleteFile();
                if (!committed && file.published) file.destination.deleteFile();
            }
        }
    } outputs;
    auto addOutput = [&](const juce::File& destination) {
        if ((!plan.stems.empty() || !plan.captures.empty()) && destination.exists()) {
            error = "Render output already exists: " + destination.getFullPathName();
            return false;
        }
        outputs.files.emplace_back();
        auto& output = outputs.files.back();
        output.destination = destination;
        output.partial = destination.getSiblingFile(destination.getFileName() + ".partial");
        output.normalized = destination.getSiblingFile(destination.getFileName() + ".normalized");
        output.partial.deleteFile();
        output.normalized.deleteFile();
        destination.deleteFile();
        output.writer = createWriter(output.partial, plan.sampleRate, error);
        if (output.writer == nullptr) return false;
        return true;
    };
    if (!plan.captures.empty()) {
        for (const auto& capture : plan.captures)
            if (!addOutput(juce::File(capture.destination))) return false;
    } else if (plan.stems.empty()) {
        if (!addOutput(plan.destination)) return false;
    } else {
        for (const auto& stem : plan.stems)
            if (!addOutput(juce::File(stem.destination))) return false;
    }

    juce::AudioBuffer<float> buffer(2, plan.blockSize);
    // Latency compensation shifts produced audio later in the callback stream, so the write
    // window in engine-sample space is the requested timeline range moved by the latency.
    const auto writeBegin = plan.startSample + plan.latencySamples;
    const auto writeEnd = plan.outputEndSample + plan.latencySamples;
    std::uint64_t writtenFrames = 0;
    // Cumulative samples the production loop has processed, counted from sample 0 of the graph.
    std::int64_t position = 0;
    {
        const bool realtimePacing = plan.renderMode == RenderMode::realtime;
        // The pacing clock starts immediately before the production loop: the warm-up and the
        // plugin reset would otherwise count as render time and let the first blocks burst.
        const auto pacedStart = std::chrono::steady_clock::now();
        const PacingWaiter pacingWaiter;
        while (position < plan.processingEnd) {
            if (isCancelled()) {
                error = "Offline Render was cancelled.";
                return false;
            }
            const auto count = static_cast<int>(
                std::min<std::int64_t>(plan.blockSize, plan.processingEnd - position));
            buffer.clear();
            engine->mix(buffer.getArrayOfWritePointers(), 2, count);
            if (plan.stems.empty())
                engine->applyMasterGain(buffer.getArrayOfWritePointers(), 2, count,
                                        plan.masterGainDb);
            const auto stop = std::min(writeEnd, position + count);
            if (stop > writeBegin) {
                const auto from = std::max(writeBegin, position);
                const auto offsetInBlock = static_cast<int>(from - position);
                const auto writeCount = static_cast<int>(stop - from);
                for (std::size_t i = 0; i < outputs.files.size(); ++i) {
                    auto& output = outputs.files[i];
                    const auto master = !plan.captures.empty() &&
                                        plan.captures[i].target.kind == OfflineCaptureKind::master;
                    const auto& source = master || (plan.stems.empty() && plan.captures.empty())
                                             ? buffer
                                             : engine->captureBuffer(i);
                    for (int channel = 0; channel < 2; ++channel)
                        output.peak = std::max(
                            output.peak, source.getMagnitude(channel, offsetInBlock, writeCount));
                    if (!output.writer->writeFromAudioSampleBuffer(source, offsetInBlock,
                                                                   writeCount)) {
                        error = "Offline Render WAV could not be written: " +
                                output.destination.getFullPathName();
                        return false;
                    }
                }
                writtenFrames += static_cast<std::uint64_t>(writeCount);
            }
            position += count;
            // Pacing is measured by the samples this loop produced, not by the timeline position,
            // so a range that starts later than zero paces from its first block.
            if (realtimePacing && !waitForPacing(pacingWaiter, pacedStart, position)) {
                error = "Offline Render was cancelled.";
                return false;
            }
        }
    }
    for (auto& output : outputs.files) output.writer.reset();

    // Every stream sample of the requested range must reach the writer; latency shifts the
    // window, it never shrinks it.
    const auto expectedFrames = static_cast<std::uint64_t>(plan.outputEndSample - plan.startSample);
    if (writtenFrames != expectedFrames) {
        error = "Offline Render WAV frame count does not match the requested range.";
        return false;
    }

    for (auto& output : outputs.files) {
        const auto normalizationGain =
            plan.normalize && output.peak > 0.0f ? 0.98f / output.peak : 1.0f;
        if (plan.normalize && std::abs(normalizationGain - 1.0f) > 0.000001f) {
            if (!normalizeFile(output.partial, output.normalized, formats, normalizationGain,
                               error) ||
                !output.normalized.moveFileTo(output.destination)) {
                if (error.isEmpty())
                    error = "Offline Render normalized WAV could not be finalized.";
                return false;
            }
        } else if (!output.partial.moveFileTo(output.destination)) {
            error = "Offline Render WAV could not be finalized.";
            return false;
        }
        output.published = true;
    }
    outputs.committed = true;

    result.frames = writtenFrames;
    result.sampleRate = plan.sampleRate;
    result.automationWarnings = plan.automationWarnings;
    return true;
}

}  // namespace riffra
