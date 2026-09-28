#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <fstream>
#include <limits>
#include <memory>
#include <thread>
#include <utility>
#include <vector>

#include "../support/TestAudioProcessor.h"
#include "../support/TestSupport.h"
#include "SonalloyTestSupport.h"
#include "audio/AudioRenderPipeline.h"
#include "contract/ExecutionGraph.h"
#include "contract/ExecutionGraphDecoder.h"
#include "instruments/Vst3InstrumentRuntime.h"
#include "recording/ArrangeRecordingSession.h"
#include "recording/ArrangementCaptureSink.h"
#include "render/OfflineRenderer.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

TimelineSnapshotSpec makeTestSnapshot() {
    TimelineSnapshotSpec snapshot;
    snapshot.projectId = "test-project";
    snapshot.revision = 1;
    snapshot.graph.timebase = {960, 120.0, 4, 4};
    return snapshot;
}

TrackSpec makeAudioTrack(const juce::String& id) {
    TrackSpec track;
    track.id = id;
    track.kind = TrackKindSpec::audio;
    return track;
}

TrackSpec makeInstrumentTrack(const juce::String& id) {
    TrackSpec track;
    track.id = id;
    track.kind = TrackKindSpec::instrument;
    return track;
}

bool loadTestSnapshot(TimelineEngine& engine, const TimelineSnapshotSpec& snapshot,
                      juce::AudioFormatManager& formats, const double sampleRate,
                      const int blockSize, juce::String& error,
                      const bool commitImmediately = true) {
    return engine.loadSnapshot(snapshot, formats, sampleRate, blockSize, error, commitImmediately);
}

bool renderTestSnapshot(const ExecutionGraph& graph, juce::AudioFormatManager& formats,
                        const juce::File& destination, const std::uint64_t startTick,
                        const std::uint64_t endTick, const std::uint32_t sampleRate,
                        const std::uint32_t blockSize, const bool normalize,
                        OfflineRenderer::Result& result, juce::String& error) {
    const OfflineRenderRequestSpec request{
        graph, destination.getFullPathName(), startTick, endTick, sampleRate, blockSize, normalize};
    const auto renderer = OfflineRenderer::prepare(request, formats, error);
    return renderer != nullptr && renderer->render(formats, result, error);
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
    void writeAudioTrack(const juce::String& trackId, const float* raw,
                         const int rawSampleCount) noexcept override {
        receivedTrack = trackId;
        receivedSamples += rawSampleCount;
        currentRawSamples += std::max(0, rawSampleCount);
        totalRawSamples += std::max(0, rawSampleCount);
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
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
    void writeAudioTrack(const juce::String&, const float* raw,
                         int rawSampleCount) noexcept override {
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
        totalRaw += std::max(0, rawSampleCount);
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
    int maxOfflineProcessedWriteSize = 0;
    int offlineProcessedWriteCalls = 0;

private:
    juce::File testDirectory;
    juce::String fileName;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> segmentRanges;
    std::uint64_t segmentStartSample = 0;
};

}  // namespace

namespace {

bool finalizeCapturedRecording(TimelineEngine& engine, juce::String& error) {
    if (!engine.stopRecording(error) || !engine.finalizeRecording(error)) return false;
    if (!engine.stop()) return false;
    return engine.processFinalizedRecording(error);
}

TimelineSnapshotSpec makeInstrumentSnapshot(const juce::String& trackId) {
    auto snapshot = makeTestSnapshot();
    snapshot.graph.tracks.push_back(makeInstrumentTrack(trackId));
    return snapshot;
}

TimelineSnapshotSpec makeBuiltInInstrumentSnapshot(const juce::String& trackId,
                                                   const bool loopEnabled = false,
                                                   const bool armed = false) {
    auto snapshot = makeInstrumentSnapshot(trackId);
    const auto preset = test::builtInPresetDirectory("BASS-001");
    snapshot.graph.loopRange = {loopEnabled, 0, loopEnabled ? 240u : 0u};
    auto& track = snapshot.graph.tracks.front();
    track.armed = armed;
    track.instrument = InternalInstrumentSpec{
        "instrument:clean-sub-bass", false,
        preset.getChildFile("definition.json").loadFileAsString(), preset.getFullPathName()};
    track.midiClips.push_back(MidiClipSpec{"midi:builtin",
                                           0,
                                           loopEnabled ? 240u : 960u,
                                           loopEnabled,
                                           false,
                                           {{0, 120, 60, 100, 1}},
                                           {}});
    return snapshot;
}

TimelineSnapshotSpec makeAudioTrackSnapshot(const int trackCount, const bool monitorFirstTrack,
                                            const bool armFirstTrack) {
    auto snapshot = makeTestSnapshot();
    for (int index = 0; index < trackCount; ++index) {
        const auto primary = index == 0;
        auto track =
            makeAudioTrack(primary ? juce::String("track:live")
                                   : juce::String("track:unrelated-") + juce::String(index));
        track.armed = primary && armFirstTrack;
        track.monitoring = primary && monitorFirstTrack ? MonitoringSpec::on : MonitoringSpec::off;
        track.audioInput = AudioInputSpec{0};
        snapshot.graph.tracks.push_back(std::move(track));
    }
    return snapshot;
}

TimelineSnapshotSpec makeRawAndProcessedClipSnapshot(const juce::File& rawFile,
                                                     const juce::File& processedFile,
                                                     const std::uint64_t sourceFrames = 32) {
    auto snapshot = makeTestSnapshot();
    auto track = makeAudioTrack("track:audio");
    const auto makeClip = [sourceFrames](const juce::String& id, const juce::File& file,
                                         const TakeVariantSpec takeVariant) {
        return AudioClipSpec{
            id, file.getFullPathName(),    48'000, 0,   sourceFrames, sourceFrames, 48'000, 0, 0,
            0,  FadeShapeSpec::equalPower, 0.0,    0.0, takeVariant,  false,        false};
    };
    track.audioClips.push_back(makeClip("clip:raw", rawFile, TakeVariantSpec::raw));
    track.audioClips.push_back(
        makeClip("clip:processed", processedFile, TakeVariantSpec::processed));
    snapshot.graph.tracks.push_back(std::move(track));
    return snapshot;
}
}  // namespace

class TimelineEngineTestPeer final {
public:
    static bool addChainDevice(PluginChain& chain, const juce::String& id,
                               std::unique_ptr<juce::AudioProcessor> processor,
                               const double sampleRate, const int blockSize, juce::String& error) {
        auto rack = PluginRackTestPeer::install(std::move(processor), sampleRate, blockSize, error);
        if (rack == nullptr) return false;
        chain.devices.push_back(PluginChain::Device{id, std::move(rack)});
        chain.prepare(sampleRate, blockSize);
        return true;
    }

    static bool installTrackChainDevice(TimelineEngine& engine, const juce::String& trackId,
                                        const juce::String& deviceId,
                                        std::unique_ptr<juce::AudioProcessor> processor,
                                        const double sampleRate, const int blockSize,
                                        juce::String& error) {
        auto* const graph = committedGraph(engine);
        if (graph == nullptr) return false;
        const auto found =
            std::find_if(graph->tracks.begin(), graph->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == graph->tracks.end()) return false;
        auto& track = *(*found);
        return track.runtime != nullptr &&
               addChainDevice(track.runtime->effects(), deviceId, std::move(processor), sampleRate,
                              blockSize, error);
    }

    static bool installTrackInstrument(TimelineEngine& engine, const juce::String& trackId,
                                       const juce::String& deviceId,
                                       std::unique_ptr<PluginRack> rack) {
        auto* const graph = committedGraph(engine);
        if (graph == nullptr) return false;
        const auto found =
            std::find_if(graph->tracks.begin(), graph->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == graph->tracks.end() || (*found)->runtime == nullptr) return false;
        auto& track = *(*found);
        track.instrumentDeviceId = deviceId;
        track.instrument = Vst3InstrumentSpec{deviceId, "test-instrument.vst3", {}};
        track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        return true;
    }

    static bool setPlaybackCompensationForTest(TimelineEngine& engine, const juce::String& trackId,
                                               const std::int64_t samples) {
        auto* const graph = committedGraph(engine);
        if (graph == nullptr) return false;
        const auto found =
            std::find_if(graph->tracks.begin(), graph->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == graph->tracks.end()) return false;
        auto& track = *(*found);
        if (track.runtime == nullptr) return false;
        track.runtime->compensationDelaySamples = samples;
        track.runtime->postEffectCompensationDelaySamples = samples;
        const auto bufferSize = static_cast<int>(samples + track.runtime->preparedBlockSize + 1);
        track.runtime->delayBuffer.setSize(2, bufferSize, false, true, false);
        track.runtime->delayBuffer.clear();
        track.runtime->postEffectDelayBuffer.setSize(2, bufferSize, false, true, false);
        track.runtime->postEffectDelayBuffer.clear();
        return true;
    }

    static bool cachePluginTailForTest(TimelineEngine& engine, const juce::String& trackId) {
        auto* const graph = committedGraph(engine);
        if (graph == nullptr) return false;
        const auto found =
            std::find_if(graph->tracks.begin(), graph->tracks.end(),
                         [&trackId](const auto& item) { return item->id == trackId; });
        if (found == graph->tracks.end() || (*found)->runtime == nullptr) return false;
        auto& runtime = *(*found)->runtime;
        runtime.pluginTailSamples = runtime.totalPluginTailSamples();
        return true;
    }

    static PreparedTimeline* committedGraph(TimelineEngine& engine) {
        return engine.graphRegistry.access(
            [](ControlGraphRegistry::State& graphs) { return graphs.latestCommitted; });
    }

    static PreparedTimeline* pendingGraph(TimelineEngine& engine) {
        return engine.graphRegistry.access(
            [](ControlGraphRegistry::State& graphs) { return graphs.pending.get(); });
    }

    /// Opens one block as the audio callback does and reports its capture window.
    static bool recordingWindow(TimelineEngine& engine, const int sampleCount, int& captureOffset,
                                int& capturedSamples) {
        (void)engine.beginBlock(sampleCount);
        captureOffset = engine.realtime.captureBlockOffset;
        capturedSamples = engine.realtime.captureBlockSamples;
        return capturedSamples > 0;
    }

    static bool trackEffectChainProcessesOnce() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:effect-chain"), formats,
                              48'000.0, 32, error))
            return false;
        std::vector<int> processOrder;
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& track = *graph->tracks.front();
            if (track.runtime == nullptr ||
                !addChainDevice(track.runtime->effects(), "effect:first",
                                std::make_unique<TestChainProcessor>(1, 1.0f, 0, processOrder),
                                48'000.0, 32, error) ||
                !addChainDevice(track.runtime->effects(), "effect:second",
                                std::make_unique<TestChainProcessor>(2, 1.0f, 0, processOrder),
                                48'000.0, 32, error))
                return false;
        }

        // Act
        if (!engine.play()) return false;
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return processOrder == std::vector<int>{1, 2};
    }

    static bool canonicalTrackStateSurvivesReusableDeviceCommit() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        const auto first = makeInstrumentSnapshot("track:state");
        if (!loadTestSnapshot(engine, first, formats, 48'000.0, 32, error)) return false;
        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        if (!TimelineEngineTestPeer::installTrackInstrument(engine, "track:state",
                                                            "instrument:state", std::move(rack)))
            return false;

        auto second = makeInstrumentSnapshot("track:state");
        auto& secondTrack = second.graph.tracks.front();
        second.revision = 2;
        secondTrack.gainDb = -6.0;
        secondTrack.pan = 0.5;
        secondTrack.muted = true;
        secondTrack.solo = true;
        secondTrack.armed = true;
        secondTrack.instrument = Vst3InstrumentSpec{
            "instrument:state", "test-instrument.vst3", {std::nullopt, {}, false}};
        secondTrack.midiClips.push_back(
            MidiClipSpec{"midi:test", 0, 960, false, false, {{0, 120, 64, 100, 1}}, {}});
        secondTrack.volumeAutomation.push_back({480, -3.0});

        // Act
        if (!engine.loadSnapshot(second, formats, 48'000.0, 32, error, false) ||
            !engine.preparedTrackReusesRuntimeDevices("track:state") ||
            !(engine.commitPreparedSnapshot(error) == RealtimeRequest::accepted))
            return false;

        // Assert
        auto* const graph = committedGraph(engine);
        if (graph == nullptr || graph->tracks.size() != 1) return false;
        const auto& runtime = *graph->tracks.front()->runtime;
        return runtime.instrument() != nullptr && runtime.instrument()->vst3Rack() == rackPointer &&
               runtime.gainDb == -6.0f && runtime.pan == 0.5f && runtime.muted && runtime.solo &&
               runtime.armed && runtime.midiClips.size() == 1 && !runtime.volumeAutomation.empty();
    }

    static bool editorParameterUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:editor-instrument"), formats,
                              48'000.0, 32, error))
            return false;
        auto rack = PluginRackTestPeer::installInstrument(std::make_unique<StateTestProcessor>(),
                                                          48'000.0, 32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& track = *graph->tracks.front();
            if (track.runtime == nullptr) return false;
            track.instrumentDeviceId = "instrument:editor";
            track.instrument = Vst3InstrumentSpec{"instrument:editor", "test-instrument.vst3", {}};
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        if (!engine.mirrorEditorDeviceParameter("track:editor-instrument", "instrument:editor", 0,
                                                0.75f, error))
            return false;
        if (!engine.play()) return false;
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        const auto state = rackPointer->persistedState(error);
        return state.has_value() && !state->parameterValues.empty() &&
               std::abs(state->parameterValues[0] - 0.75f) <= 0.0001f;
    }

    static bool persistedStateUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:plugin-state"), formats,
                              48'000.0, 32, error))
            return false;

        auto rack = PluginRackTestPeer::installInstrument(std::make_unique<StateTestProcessor>(),
                                                          48'000.0, 32, error);
        if (rack == nullptr) return false;
        auto* rackPointer = rack.get();
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& track = *graph->tracks.front();
            if (track.runtime == nullptr) return false;
            track.instrumentDeviceId = "instrument:plugin-state";
            track.instrument =
                Vst3InstrumentSpec{"instrument:plugin-state", "test-instrument.vst3", {}};
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }
        if (!rackPointer->setParameter(0, 0.25f, error)) return false;

        // Act
        const auto changed =
            engine.setDevicePersistedState("track:plugin-state", "instrument:plugin-state",
                                           PluginStateSpec{std::nullopt, {0.75f}, false}, error);

        // Assert
        if (!changed) return false;
        const auto restored = rackPointer->persistedState(error);
        return restored.has_value() && !restored->parameterValues.empty() &&
               std::abs(restored->parameterValues[0] - 0.75f) <= 0.0001f;
    }

    static bool programChangeUpdatesInstrumentRuntime() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:plugin-program"), formats,
                              48'000.0, 32, error))
            return false;

        ProcessorTrace trace;
        auto rack = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), 48'000.0,
                                                32, error);
        if (rack == nullptr) return false;
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& track = *graph->tracks.front();
            if (track.runtime == nullptr) return false;
            track.instrumentDeviceId = "instrument:plugin-program";
            track.instrument =
                Vst3InstrumentSpec{"instrument:plugin-program", "test-instrument.vst3", {}};
            track.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        const auto changed =
            engine.setDeviceProgram("track:plugin-program", "instrument:plugin-program", 1, error);

        // Assert
        return changed && trace.currentProgram == 1;
    }

    static bool liveInstrumentProcessesWhileStopped() {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:live-instrument"), formats,
                              48'000.0, 32, error))
            return false;

        InstrumentTrace trace;
        auto instrumentRack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (instrumentRack == nullptr) return false;
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& liveTrack = *graph->tracks.front();
            if (liveTrack.runtime == nullptr) return false;
            liveTrack.runtime->setInstrument(
                Vst3InstrumentRuntime::fromRack(std::move(instrumentRack)));
            // Simulate a Project where another Track's plugin is the latency
            // leader while the Play Surface targets this Instrument Track.
            liveTrack.runtime->pluginDelaySamples = 0;
            liveTrack.runtime->compensationDelaySamples = 4;
        }

        if (!(engine.setLiveMidiTarget("track:live-instrument", error) ==
              RealtimeRequest::accepted))
            return false;

        if (!(engine.enqueueTargetedMidi("track:live-instrument",
                                         juce::MidiMessage::noteOn(1, 60, 0.8f),
                                         error) == RealtimeRequest::accepted))
            return false;

        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
        const auto peak = std::max(*std::max_element(left.begin(), left.end()),
                                   *std::max_element(right.begin(), right.end()));
        // The focused Play Surface target bypasses inter-track compensation delay.
        const auto immediate = std::max(left[0], right[0]);
        return trace.lastMidiMessage.isNoteOn() && trace.noteHeld && peak > 0.0f &&
               immediate > 0.0f;
    }

    static bool pluginDevicesWarmUpAndReset() {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine(true);
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:warm-up"), formats, 48'000.0,
                              32, error))
            return false;
        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (rack == nullptr ||
            !installTrackInstrument(engine, "track:warm-up", "instrument:warm-up", std::move(rack)))
            return false;
        const auto preparedResets = trace.resetCount;

        engine.warmUpPluginDevices(32);
        const auto warmedUp = trace.processBlockCount == 1 && trace.resetCount == preparedResets;
        engine.resetPluginDevices();

        return warmedUp && trace.resetCount == preparedResets + 1;
    }

    static bool timelineMidiUsesCurrentTransportContext() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        auto snapshot = makeInstrumentSnapshot("track:pdc");
        snapshot.graph.tracks.front().midiClips.push_back(
            MidiClipSpec{"midi:pdc", 0, 960, false, false, {{0, 120, 60, 100, 1}}, {}});
        if (!loadTestSnapshot(engine, snapshot, formats, 48'000.0, 32, error)) return false;

        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (rack == nullptr || !rack->prepareTimelineMidiCapacity(2, error)) return false;
        if (!TimelineEngineTestPeer::installTrackInstrument(engine, "track:pdc", "instrument:pdc",
                                                            std::move(rack)))
            return false;
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.empty()) return false;
            graph->tracks.front()->runtime->compensationDelaySamples = 4;
        }
        if (!(engine.enqueueTargetedMidi("track:pdc", juce::MidiMessage::noteOn(1, 72, 0.8f),
                                         error) == RealtimeRequest::accepted))
            return false;

        // Act
        if (!engine.play()) return false;
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        const std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return trace.midiSamplePositions.size() >= 2 && trace.midiSamplePositions[0] == 0 &&
               trace.midiSamplePositions[1] == 0;
    }

    static bool panicClosesInstrumentRuntime() {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        if (!loadTestSnapshot(engine, makeInstrumentSnapshot("track:panic-instrument"), formats,
                              48'000.0, 32, error))
            return false;

        InstrumentTrace trace;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), 48'000.0, 32, error);
        if (rack == nullptr) return false;
        {
            auto* const graph = committedGraph(engine);
            if (graph == nullptr || graph->tracks.size() != 1) return false;
            auto& panicTrack = *graph->tracks.front();
            if (panicTrack.runtime == nullptr) return false;
            panicTrack.runtime->setInstrument(Vst3InstrumentRuntime::fromRack(std::move(rack)));
        }

        // Act
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        if (!engine.panicAllInstrumentTracks()) return false;
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));
        if (!engine.play()) return false;
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        return trace.midiMessages.size() == 48u;
    }

    static bool audioDeviceRestartRebuildsRuntimeFormat() {
        // Arrange
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        juce::String error;
        const auto snapshot = makeInstrumentSnapshot("track:audio-device");
        if (!loadTestSnapshot(engine, snapshot, formats, 48'000.0, 256, error) ||
            !loadTestSnapshot(engine, snapshot, formats, 48'000.0, 256, error, false) ||
            !engine.preparedTrackReusesRuntimeDevices("track:audio-device") ||
            !(engine.commitPreparedSnapshot(error) == RealtimeRequest::accepted))
            return false;

        // Act
        engine.audioDeviceStarted();
        if (!loadTestSnapshot(engine, snapshot, formats, 44'100.0, 1024, error, false))
            return false;

        double preparedSampleRate = 0.0;
        int preparedBlockSize = 0;
        double trackSampleRate = 0.0;
        bool reusesRuntimeDevices = true;
        {
            auto* const pending = pendingGraph(engine);
            if (pending == nullptr || pending->tracks.empty()) return false;
            preparedSampleRate = pending->outputSampleRate;
            preparedBlockSize = pending->preparedBlockSize;
            trackSampleRate = pending->tracks.front()->runtime->outputSampleRate;
            reusesRuntimeDevices = pending->tracks.front()->reuseRuntimeDevices;
        }
        if (reusesRuntimeDevices || std::abs(preparedSampleRate - 44'100.0) > 0.1 ||
            preparedBlockSize != 1024 || std::abs(trackSampleRate - 44'100.0) > 0.1)
            return false;

        // Assert
        if (!(engine.commitPreparedSnapshot(error) == RealtimeRequest::accepted)) return false;
        (void)engine.beginBlock(1024);
        const auto status = engine.status();
        return status.graph.has_value() && std::abs(status.graph->sampleRate - 44'100.0) <= 0.1;
    }

    static juce::var run(const juce::File& directory) {
        auto* result = new juce::DynamicObject();
        result->setProperty("type", "timelineSelfTest");
        juce::Array<juce::var> checks;
        const auto mono = directory.getChildFile("timeline-44100-mono.wav");
        const auto stereo = directory.getChildFile("timeline-48000-stereo.wav");
        directory.createDirectory();
        const auto sourcesWritten = writePcmWave(mono, 44100, 1, 44100, 6000) &&
                                    writePcmWave(stereo, 48000, 2, 48000, 4000);

        bool loaded = false;
        bool mixed = false;
        bool seeked = false;
        bool looped = false;
        bool punchWindowed = false;
        bool immediateRecordStarted = false;
        bool countInAligned = false;
        bool countInAudible = false;
        bool countInCancelled = false;
        bool metronomeMixed = false;
        bool automationRamped = false;
        bool offlineRangeRendered = false;
        bool offlineAudioRendered = false;
        bool offlineNormalized = false;
        bool graphUpdateReusedDevices = false;
        bool mutablePluginStateKeepsTopology = false;
        bool recordingTapIsolated = false;
        bool loopCaptureSegments = false;
        bool syntheticLoopPassed = false;
        bool partialPassPassed = false;
        bool blockSizePassed = false;
        bool longRecordingPassed = false;
        bool productionWriterPassed = false;
        bool productionWriterPartialPassed = false;
        const auto liveInstrumentWhileStopped = liveInstrumentProcessesWhileStopped();
        const auto panicClosesRuntime = panicClosesInstrumentRuntime();
        int diagPartialSegments = 0;
        int diagPartialRaw = 0;
        int diagPartialProcessed = 0;
        int diagPartialWindowed = 0;
        int diagBsRaw = 0;
        int diagBsProcessed = 0;
        int diagBsWindowed = 0;
        int diagPartialFailIndex = -1;
        float diagPartialFailValue = 0.0f;
        float diagPartialRawAtFail = 0.0f;
        float automationEarlyLeft = 0.0f;
        float automationEarlyRight = 0.0f;
        float automationLateLeft = 0.0f;
        float automationLateRight = 0.0f;
        std::uint64_t diagProductionRawSamples = 0;
        std::uint64_t diagProductionProcessedSamples = 0;
        std::uint64_t diagProductionMissing = 0;
        std::uint64_t diagProductionDropped = 0;
        std::uint64_t diagProductionPartialRaw = 0;
        std::uint64_t diagProductionPartialProcessed = 0;
        juce::String error;
        {
            const auto first = PluginDeviceSpec{
                "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.1f, 0.2f}, false}};
            const auto second = PluginDeviceSpec{
                "device:test", "C:\\test\\Effect.vst3", {std::nullopt, {0.8f, 0.9f}, true}};
            mutablePluginStateKeepsTopology = sameEffectTopology(
                std::vector<PluginDeviceSpec>{first}, std::vector<PluginDeviceSpec>{second});
        }
        if (sourcesWritten) {
            juce::AudioFormatManager formats;
            formats.registerBasicFormats();
            TimelineEngine engine;
            auto snapshot = makeTestSnapshot();
            snapshot.revision = 7;
            auto audioTrack = makeAudioTrack("track:test");
            audioTrack.armed = true;
            audioTrack.audioInput = AudioInputSpec{0};
            audioTrack.audioClips = {
                {"mono-44100", mono.getFullPathName(), 44'100, 0, 44'100, 44'100, 44'100, 0, 0, 0,
                 FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false},
                {"stereo-48000", stereo.getFullPathName(), 48'000, 0, 48'000, 48'000, 48'000, 0, 0,
                 0, FadeShapeSpec::linear, 0.0, 0.0, TakeVariantSpec::raw, false, false}};
            audioTrack.volumeAutomation = {{0, -24.0}, {20, 0.0}};
            audioTrack.panAutomation = {{0, -1.0}, {20, 1.0}};
            snapshot.graph.tracks.push_back(std::move(audioTrack));
            auto& activeAudioTrack = snapshot.graph.tracks.front();
            loaded = loadTestSnapshot(engine, snapshot, formats, 48000.0, 512, error);
            if (loaded) {
                snapshot.revision = 8;
                activeAudioTrack.gainDb = -3.0;
                graphUpdateReusedDevices =
                    loadTestSnapshot(engine, snapshot, formats, 48000.0, 512, error, false) &&
                    engine.preparedTrackReusesRuntimeDevices("track:test") &&
                    (engine.commitPreparedSnapshot(error) == RealtimeRequest::accepted);
                OfflineRenderer::Result offlineResult;
                const auto offlineOutput = directory.getChildFile("offline-selection.wav");
                if (renderTestSnapshot(snapshot.graph, formats, offlineOutput, 480, 1440, 48000,
                                       512, false, offlineResult, error)) {
                    auto reader = std::unique_ptr<juce::AudioFormatReader>(
                        formats.createReaderFor(offlineOutput));
                    juce::AudioBuffer<float> rendered(2, 24000);
                    offlineRangeRendered = reader != nullptr && reader->numChannels == 2 &&
                                           reader->lengthInSamples == 24000;
                    offlineAudioRendered = offlineRangeRendered &&
                                           reader->read(&rendered, 0, 24000, 0, true, true) &&
                                           std::max(rendered.getMagnitude(0, 0, 24000),
                                                    rendered.getMagnitude(1, 0, 24000)) > 0.01f;
                }
                OfflineRenderer::Result normalizedResult;
                const auto normalizedOutput = directory.getChildFile("offline-normalized.wav");
                if (renderTestSnapshot(snapshot.graph, formats, normalizedOutput, 0, 1440, 48000,
                                       512, true, normalizedResult, error)) {
                    auto reader = std::unique_ptr<juce::AudioFormatReader>(
                        formats.createReaderFor(normalizedOutput));
                    if (reader != nullptr && reader->numChannels == 2 &&
                        reader->lengthInSamples > 0) {
                        const auto sampleCount = static_cast<int>(reader->lengthInSamples);
                        juce::AudioBuffer<float> rendered(2, sampleCount);
                        if (reader->read(&rendered, 0, sampleCount, 0, true, true)) {
                            const auto peak = std::max(rendered.getMagnitude(0, 0, sampleCount),
                                                       rendered.getMagnitude(1, 0, sampleCount));
                            offlineNormalized = std::abs(peak - 0.98f) <= 0.02f;
                        }
                    }
                }
                if (!engine.seekToTick(0)) return false;
                if (!engine.play()) return false;
                std::array<float, 512> left{};
                std::array<float, 512> right{};
                std::array<float*, 2> channels{left.data(), right.data()};
                engine.mix(channels.data(), 2, static_cast<int>(left.size()));
                automationEarlyLeft = std::abs(left[20]);
                automationEarlyRight = std::abs(right[20]);
                automationLateLeft = std::abs(left[490]);
                automationLateRight = std::abs(right[490]);
                automationRamped = std::abs(left[20]) > std::abs(right[20]) * 2.0f &&
                                   std::abs(right[490]) > std::abs(left[490]) * 2.0f &&
                                   std::abs(left[490]) + std::abs(right[490]) >
                                       (std::abs(left[20]) + std::abs(right[20])) * 4.0f;
                for (int block = 1; block < 8; ++block)
                    engine.mix(channels.data(), 2, static_cast<int>(left.size()));
                const auto peak = std::max(*std::max_element(left.begin(), left.end()),
                                           *std::max_element(right.begin(), right.end()));
                mixed = peak > 0.1f;
                if (!engine.seekToTick(960)) return false;
                const auto seekStatus = engine.status();
                seeked = seekStatus.frame.timelineSample == 24000;

                CaptureIsolationSink captureSink(directory);
                engine.setRecordingSink(&captureSink);
                if (!engine.seekToTick(0)) return false;
                int captureOffset = 0;
                int captureSamples = 0;
                std::array<float, 512> physicalInput{};
                physicalInput.fill(0.05f);
                std::array<float, 512> captureLeft{};
                std::array<float, 512> captureRight{};
                const std::array<const float*, 1> physicalInputs{physicalInput.data()};
                const std::array<float*, 2> captureOutputs{captureLeft.data(), captureRight.data()};
                const auto captureStarted =
                    (engine.startRecording(0, error) == RealtimeRequest::accepted);
                const auto captureWindow =
                    captureStarted &&
                    recordingWindow(engine, static_cast<int>(physicalInput.size()), captureOffset,
                                    captureSamples);
                if (captureWindow)
                    engine.mix(physicalInputs.data(), 1, captureOutputs.data(), 2,
                               static_cast<int>(physicalInput.size()));
                if (!engine.stopRecording(error)) return false;
                const auto captureFinalized = finalizeCapturedRecording(engine, error);
                engine.clearRecordingSink();
                recordingTapIsolated =
                    captureWindow && captureFinalized && captureOffset == 0 &&
                    captureSamples == static_cast<int>(physicalInput.size()) &&
                    captureSink.receivedTrack == "track:test" &&
                    captureSink.receivedSamples == static_cast<int>(physicalInput.size()) &&
                    captureSink.totalProcessedSamples == static_cast<int>(physicalInput.size()) &&
                    captureSink.offlineProcessedWriteCalls > 0;

                auto loopSnapshot = makeTestSnapshot();
                loopSnapshot.revision = 8;
                loopSnapshot.graph.loopRange = {true, 0, 960};
                loopSnapshot.graph.metronomeEnabled = true;
                auto loopTrack = makeAudioTrack("track:loop");
                loopTrack.armed = true;
                loopTrack.audioInput = AudioInputSpec{0};
                loopSnapshot.graph.tracks.push_back(std::move(loopTrack));
                auto punchSnapshot = loopSnapshot;
                punchSnapshot.graph.punchRange = TickRangeSpec{480, 960};
                const auto loopSnapshotLoaded =
                    loadTestSnapshot(engine, loopSnapshot, formats, 48000.0, 512, error);
                if (loopSnapshotLoaded) {
                    CaptureIsolationSink loopCaptureSink(directory);
                    engine.setRecordingSink(&loopCaptureSink);
                    if (!engine.seekToTick(0)) return false;
                    int loopCaptureOffset = 0;
                    int loopCaptureSamples = 0;
                    constexpr int loopPassSamples = 24'000;
                    constexpr int loopBlockSamples = 512;
                    constexpr int loopTotalSamples = loopPassSamples * 3;
                    std::array<float, loopBlockSamples> loopAudioInput{};
                    loopAudioInput.fill(0.05f);
                    std::array<float, loopBlockSamples> loopOutputLeft{};
                    std::array<float, loopBlockSamples> loopOutputRight{};
                    const std::array<const float*, 1> loopInputs{loopAudioInput.data()};
                    const std::array<float*, 2> loopOutputs{loopOutputLeft.data(),
                                                            loopOutputRight.data()};
                    const auto loopRecordingStarted =
                        (engine.startRecording(0, error) == RealtimeRequest::accepted);
                    const auto loopWindowed =
                        loopRecordingStarted &&
                        recordingWindow(engine, loopTotalSamples, loopCaptureOffset,
                                        loopCaptureSamples);
                    auto loopRemaining = loopTotalSamples;
                    while (loopWindowed && loopRemaining > loopBlockSamples) {
                        engine.mix(loopInputs.data(), 1, loopOutputs.data(), 2, loopBlockSamples);
                        loopRemaining -= loopBlockSamples;
                    }
                    if (loopWindowed && loopRemaining > 0)
                        engine.mix(loopInputs.data(), 1, loopOutputs.data(), 2, loopRemaining);
                    if (!engine.stopRecording(error)) return false;
                    finalizeCapturedRecording(engine, error);
                    engine.clearRecordingSink();
                    loopCaptureSegments =
                        loopWindowed && loopCaptureOffset == 0 &&
                        loopCaptureSamples == loopTotalSamples && loopCaptureSink.beginCount == 3 &&
                        loopCaptureSink.endCount == 3 && loopCaptureSink.loopBoundaryCount == 3 &&
                        loopCaptureSink.totalRawSamples == loopTotalSamples &&
                        loopCaptureSink.totalProcessedSamples == loopTotalSamples &&
                        loopCaptureSink.offlineProcessedWriteCalls > 1 &&
                        loopCaptureSink.maxOfflineProcessedWriteSize <= loopBlockSamples;
                    for (int index = 0; loopCaptureSegments && index < 3; ++index) {
                        const auto offset = static_cast<std::size_t>(index);
                        loopCaptureSegments =
                            loopCaptureSink.segmentRawSamples[offset] > 0 &&
                            loopCaptureSink.endAudioSamples[offset] >
                                loopCaptureSink.beginAudioSamples[offset] &&
                            (index == 0 || loopCaptureSink.beginAudioSamples[offset] >=
                                               loopCaptureSink.endAudioSamples[offset - 1]) &&
                            loopCaptureSink.beginTimelineSamples[offset] == 0 &&
                            loopCaptureSink.endTimelineSamples[offset] ==
                                static_cast<std::uint64_t>(loopPassSamples);
                    }
                }
                // Synthetic loop recording test with distinct impulses per pass.
                if (loopSnapshotLoaded &&
                    loadTestSnapshot(engine, loopSnapshot, formats, 48000.0, 512, error)) {
                    constexpr int kSynthLoopLength = 24'000;
                    constexpr int kSynthPasses = 3;
                    constexpr int kSynthTotal = kSynthLoopLength * kSynthPasses;
                    constexpr int kSynthBlock = 512;
                    constexpr float kImpulseAmplitude = 0.9f;
                    // Impulse positions within each pass (must be >= kSynthDelay)
                    constexpr int kImpulsePos[kSynthPasses] = {256, 1256, 2256};
                    LoopDataCaptureSink synthSink(directory, "synth");
                    engine.setRecordingSink(&synthSink);
                    if (!engine.seekToTick(0)) return false;
                    int synthOffset = 0;
                    int synthSamples = 0;
                    const auto synthStarted =
                        (engine.startRecording(0, error) == RealtimeRequest::accepted);
                    const auto synthWindowed =
                        synthStarted &&
                        recordingWindow(engine, kSynthTotal, synthOffset, synthSamples);
                    const auto clockBefore = engine.realtime.audioClockSample;
                    std::array<float, kSynthBlock> synthInput{};
                    std::array<float, kSynthBlock> synthOutL{};
                    std::array<float, kSynthBlock> synthOutR{};
                    const std::array<const float*, 1> synthInputs{synthInput.data()};
                    const std::array<float*, 2> synthOutputs{synthOutL.data(), synthOutR.data()};
                    int synthMixed = 0;
                    bool synthClockContinuous = true;
                    while (synthWindowed && synthMixed < kSynthTotal) {
                        const auto block = std::min(kSynthBlock, kSynthTotal - synthMixed);
                        const auto passIndex = synthMixed / kSynthLoopLength;
                        const auto posInPass = synthMixed % kSynthLoopLength;
                        synthInput.fill(0.0f);
                        if (passIndex < kSynthPasses && posInPass <= kImpulsePos[passIndex] &&
                            kImpulsePos[passIndex] < posInPass + block) {
                            synthInput[static_cast<std::size_t>(kImpulsePos[passIndex] -
                                                                posInPass)] = kImpulseAmplitude;
                        }
                        const auto prevClock = engine.realtime.audioClockSample;
                        engine.mix(synthInputs.data(), 1, synthOutputs.data(), 2, block);
                        const auto newClock = engine.realtime.audioClockSample;
                        if (newClock != prevClock + static_cast<std::uint64_t>(block))
                            synthClockContinuous = false;
                        synthMixed += block;
                    }
                    if (!engine.stopRecording(error)) return false;
                    finalizeCapturedRecording(engine, error);
                    engine.clearRecordingSink();
                    const auto clockAfter = engine.realtime.audioClockSample;
                    // Verify: audio clock advanced continuously by total mixed samples
                    const bool synthClockOk =
                        synthClockContinuous &&
                        clockAfter == clockBefore + static_cast<std::uint64_t>(synthMixed);
                    // Verify: timeline wrapped (position < loopLength after stop)
                    const auto finalPosition = engine.realtime.timelineSample;
                    const bool synthTimelineWrapped =
                        finalPosition >= 0 && finalPosition < kSynthLoopLength;
                    // Verify: 3 segments, 3 boundaries
                    const bool synthSegmentsOk = synthSink.segmentCount == kSynthPasses &&
                                                 synthSink.boundaryCount == kSynthPasses;
                    // Verify: raw and processed lengths match
                    const bool synthLengthsOk =
                        synthSink.totalRaw == kSynthTotal &&
                        synthSink.totalProcessed == kSynthTotal &&
                        static_cast<int>(synthSink.rawBuffer.size()) == kSynthTotal &&
                        static_cast<int>(synthSink.processedLeft.size()) == kSynthTotal;
                    // Verify: impulse positions and no cross-pass contamination
                    bool synthImpulseOk = true;
                    bool synthNoCrossPass = true;
                    for (int pass = 0; synthImpulseOk && pass < kSynthPasses; ++pass) {
                        const auto base = static_cast<std::size_t>(pass * kSynthLoopLength);
                        const auto impulseAt = static_cast<std::size_t>(kImpulsePos[pass]);
                        // Raw impulse present at position P
                        synthImpulseOk = base + impulseAt < synthSink.rawBuffer.size() &&
                                         std::abs(synthSink.rawBuffer[base + impulseAt] -
                                                  kImpulseAmplitude) < 0.001f;
                        // The offline chain is empty for this synthetic capture, so the
                        // processed variant preserves the raw impulse position.
                        const auto processedPos = impulseAt;
                        synthImpulseOk = synthImpulseOk &&
                                         base + processedPos < synthSink.processedLeft.size() &&
                                         std::abs(synthSink.processedLeft[base + processedPos] -
                                                  kImpulseAmplitude) < 0.001f;
                        // No other significant samples in this pass (no contamination)
                        for (int i = 0; synthNoCrossPass && i < kSynthLoopLength; ++i) {
                            if (static_cast<std::size_t>(i) == processedPos) continue;
                            const auto idx = base + static_cast<std::size_t>(i);
                            if (idx < synthSink.processedLeft.size() &&
                                std::abs(synthSink.processedLeft[idx]) > 0.001f)
                                synthNoCrossPass = false;
                        }
                    }
                    syntheticLoopPassed = synthWindowed && synthClockOk && synthTimelineWrapped &&
                                          synthSegmentsOk && synthLengthsOk && synthImpulseOk &&
                                          synthNoCrossPass &&
                                          synthSink.offlineProcessedWriteCalls > 1 &&
                                          synthSink.maxOfflineProcessedWriteSize <= kSynthBlock;
                }
                // Partial Pass test: start recording from loop middle
                if (loopSnapshotLoaded &&
                    loadTestSnapshot(engine, loopSnapshot, formats, 48000.0, 512, error)) {
                    constexpr int kPartialTotal = 60'000;  // 12000 + 24000 + 24000
                    constexpr int kPartialBlock = 512;
                    // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                    {
                        auto* const graph = committedGraph(engine);
                        if (graph != nullptr) {
                            for (auto& trackPtr : graph->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack &&
                                    trackPtr->runtime->armed) {
                                    trackPtr->runtime->pluginDelaySamples = 0;
                                    trackPtr->runtime->pluginTailSamples = 0;
                                }
                            }
                        }
                    }
                    LoopDataCaptureSink partialSink(directory, "partial");
                    engine.setRecordingSink(&partialSink);
                    if (!engine.seekToTick(480)) return false;  // tick 480 = sample 12000
                    int partialOffset = 0;
                    int partialSamples = 0;
                    const auto partialStarted =
                        (engine.startRecording(0, error) == RealtimeRequest::accepted);
                    const auto partialWindowed =
                        partialStarted &&
                        recordingWindow(engine, kPartialTotal, partialOffset, partialSamples);
                    std::array<float, kPartialBlock> partialInput{};
                    partialInput.fill(0.05f);
                    std::array<float, kPartialBlock> partialOutL{};
                    std::array<float, kPartialBlock> partialOutR{};
                    const std::array<const float*, 1> partialInputs{partialInput.data()};
                    const std::array<float*, 2> partialOutputs{partialOutL.data(),
                                                               partialOutR.data()};
                    int partialMixed = 0;
                    while (partialWindowed && partialMixed < kPartialTotal) {
                        const auto block = std::min(kPartialBlock, kPartialTotal - partialMixed);
                        engine.mix(partialInputs.data(), 1, partialOutputs.data(), 2, block);
                        partialMixed += block;
                    }
                    if (!engine.stopRecording(error)) return false;
                    finalizeCapturedRecording(engine, error);
                    engine.clearRecordingSink();
                    // Expected: 3 segments (partial 12000, full 24000, full 24000)
                    const bool partialSegmentsOk = partialSink.segmentCount == 3;
                    const bool partialLengthsOk =
                        partialSink.totalRaw == kPartialTotal &&
                        partialSink.totalProcessed == kPartialTotal &&
                        static_cast<int>(partialSink.processedLeft.size()) == kPartialTotal;
                    // Each segment independently reset (no cross-segment contamination)
                    // With constant 0.05 input and passthrough chain, all processed ≈ 0.05
                    bool partialDataOk =
                        partialSink.processedLeft.size() == static_cast<std::size_t>(kPartialTotal);
                    int partialFailIndex = -1;
                    for (int i = 0; partialDataOk && i < kPartialTotal; ++i) {
                        if (std::abs(partialSink.processedLeft[static_cast<std::size_t>(i)] -
                                     0.05f) >= 0.02f) {
                            partialDataOk = false;
                            partialFailIndex = i;
                        }
                    }
                    partialPassPassed = partialWindowed && partialSegmentsOk && partialLengthsOk &&
                                        partialDataOk &&
                                        partialSink.offlineProcessedWriteCalls > 1 &&
                                        partialSink.maxOfflineProcessedWriteSize <= kPartialBlock;
                    diagPartialSegments = partialSink.segmentCount;
                    diagPartialRaw = partialSink.totalRaw;
                    diagPartialProcessed = partialSink.totalProcessed;
                    diagPartialWindowed = partialWindowed ? 1 : 0;
                    diagPartialFailIndex = partialFailIndex;
                    if (partialFailIndex >= 0 && static_cast<std::size_t>(partialFailIndex) <
                                                     partialSink.processedLeft.size())
                        diagPartialFailValue =
                            partialSink.processedLeft[static_cast<std::size_t>(partialFailIndex)];
                    if (partialFailIndex >= 0 &&
                        static_cast<std::size_t>(partialFailIndex) < partialSink.rawBuffer.size())
                        diagPartialRawAtFail =
                            partialSink.rawBuffer[static_cast<std::size_t>(partialFailIndex)];
                }
                // Block Size test: verify processing uses small blocks
                if (loopSnapshotLoaded &&
                    loadTestSnapshot(engine, loopSnapshot, formats, 48000.0, 128, error)) {
                    // preparedBlockSize = 128; chain must be fed in <= 128 sample chunks
                    constexpr int kBsTotal = 24'000;  // 1 pass
                    constexpr int kBsBlock = 128;
                    constexpr float kBsImpulse = 0.8f;
                    constexpr int kBsImpulsePos = 500;
                    LoopDataCaptureSink bsSink(directory, "blocksize");
                    engine.setRecordingSink(&bsSink);
                    if (!engine.seekToTick(0)) return false;
                    int bsOffset = 0;
                    int bsSamples = 0;
                    const auto bsStarted =
                        (engine.startRecording(0, error) == RealtimeRequest::accepted);
                    const auto bsWindowed =
                        bsStarted && recordingWindow(engine, kBsTotal, bsOffset, bsSamples);
                    std::array<float, kBsBlock> bsInput{};
                    std::array<float, kBsBlock> bsOutL{};
                    std::array<float, kBsBlock> bsOutR{};
                    const std::array<const float*, 1> bsInputs{bsInput.data()};
                    const std::array<float*, 2> bsOutputs{bsOutL.data(), bsOutR.data()};
                    int bsMixed = 0;
                    while (bsWindowed && bsMixed < kBsTotal) {
                        bsInput.fill(0.0f);
                        if (bsMixed <= kBsImpulsePos && kBsImpulsePos < bsMixed + kBsBlock) {
                            bsInput[static_cast<std::size_t>(kBsImpulsePos - bsMixed)] = kBsImpulse;
                        }
                        engine.mix(bsInputs.data(), 1, bsOutputs.data(), 2, kBsBlock);
                        bsMixed += kBsBlock;
                    }
                    if (!engine.stopRecording(error)) return false;
                    finalizeCapturedRecording(engine, error);
                    engine.clearRecordingSink();
                    // Verify: processed length matches raw, impulse at correct position
                    const bool bsLengthOk =
                        bsSink.totalRaw == kBsTotal && bsSink.totalProcessed == kBsTotal;
                    const auto bsProcessedPos = kBsImpulsePos;
                    const bool bsImpulseOk =
                        bsProcessedPos >= 0 &&
                        static_cast<std::size_t>(bsProcessedPos) < bsSink.processedLeft.size() &&
                        std::abs(bsSink.processedLeft[static_cast<std::size_t>(bsProcessedPos)] -
                                 kBsImpulse) < 0.01f;
                    blockSizePassed = bsWindowed && bsLengthOk && bsImpulseOk &&
                                      bsSink.offlineProcessedWriteCalls > 1 &&
                                      bsSink.maxOfflineProcessedWriteSize <= kBsBlock;
                    diagBsRaw = bsSink.totalRaw;
                    diagBsProcessed = bsSink.totalProcessed;
                    diagBsWindowed = bsWindowed ? 1 : 0;
                }
                // Long Recording test: 130 passes exceeds old 128x RAM limit
                if (loopSnapshotLoaded &&
                    loadTestSnapshot(engine, loopSnapshot, formats, 48000.0, 512, error)) {
                    constexpr int kLongLoopLength = 4'800;  // short loop
                    constexpr int kLongPasses = 130;
                    constexpr int kLongTotal = kLongLoopLength * kLongPasses;
                    constexpr int kLongBlock = 512;
                    // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                    {
                        auto* const graph = committedGraph(engine);
                        if (graph != nullptr) {
                            for (auto& trackPtr : graph->tracks) {
                                if (trackPtr->runtime != nullptr &&
                                    !trackPtr->runtime->instrumentTrack &&
                                    trackPtr->runtime->armed) {
                                    trackPtr->runtime->pluginDelaySamples = 0;
                                    trackPtr->runtime->pluginTailSamples = 0;
                                }
                            }
                        }
                    }
                    // Use a snapshot with short loop (tick 0..192 = 4800 samples)
                    auto longSnapshot = loopSnapshot;
                    longSnapshot.graph.loopRange.endTick = 192;
                    if (loadTestSnapshot(engine, longSnapshot, formats, 48000.0, 512, error)) {
                        LoopDataCaptureSink longSink(directory, "longrec");
                        engine.setRecordingSink(&longSink);
                        if (!engine.seekToTick(0)) return false;
                        int longOffset = 0;
                        int longSamples = 0;
                        const auto longStarted =
                            (engine.startRecording(0, error) == RealtimeRequest::accepted);
                        const auto longWindowed =
                            longStarted &&
                            recordingWindow(engine, kLongTotal, longOffset, longSamples);
                        std::array<float, kLongBlock> longInput{};
                        longInput.fill(0.02f);
                        std::array<float, kLongBlock> longOutL{};
                        std::array<float, kLongBlock> longOutR{};
                        const std::array<const float*, 1> longInputs{longInput.data()};
                        const std::array<float*, 2> longOutputs{longOutL.data(), longOutR.data()};
                        int longMixed = 0;
                        while (longWindowed && longMixed < kLongTotal) {
                            const auto block = std::min(kLongBlock, kLongTotal - longMixed);
                            engine.mix(longInputs.data(), 1, longOutputs.data(), 2, block);
                            longMixed += block;
                        }
                        if (!engine.stopRecording(error)) return false;
                        finalizeCapturedRecording(engine, error);
                        engine.clearRecordingSink();
                        // Verify: all 130 passes recorded, raw/processed match
                        longRecordingPassed =
                            longWindowed && longSink.segmentCount == kLongPasses &&
                            longSink.totalRaw == kLongTotal &&
                            longSink.totalProcessed == kLongTotal &&
                            static_cast<int>(longSink.processedLeft.size()) == kLongTotal &&
                            longSink.offlineProcessedWriteCalls > 1 &&
                            longSink.maxOfflineProcessedWriteSize <= kLongBlock;
                    }
                    // Restore original loop range for subsequent tests
                    loopSnapshot.graph.loopRange.endTick = 960;
                }
                // Production Writer Integration Test: 4 bars x 3 passes (384,000 x 3)
                {
                    auto prodSnapshot = makeTestSnapshot();
                    prodSnapshot.revision = 10;
                    prodSnapshot.graph.loopRange = {true, 0, 15'360};
                    auto prodTrack = makeAudioTrack("track:prod");
                    prodTrack.armed = true;
                    prodTrack.audioInput = AudioInputSpec{0};
                    prodSnapshot.graph.tracks.push_back(std::move(prodTrack));

                    if (loadTestSnapshot(engine, prodSnapshot, formats, 48000.0, 512, error)) {
                        constexpr int kProdLoopLength = 384'000;
                        constexpr int kProdPasses = 3;
                        constexpr int kProdTotal = kProdLoopLength * kProdPasses;
                        constexpr int kProdBlock = 512;

                        // 3 full passes. AudioRenderPipeline owns transport stop and capture
                        // detachment; this test completes the detached offline job explicitly.
                        if (!engine.seekToTick(0)) return false;
                        auto prodDir = directory.getChildFile("prod-writer");
                        AudioRenderPipeline prodCallback(engine);
                        juce::String sessionError;
                        const auto prodArrangeStarted =
                            prodCallback.recording().start(prodDir, sessionError);
                        if (prodArrangeStarted) {
                            int prodOffset = 0;
                            int prodSamples = 0;
                            const auto prodStarted =
                                (engine.startRecording(0, error) == RealtimeRequest::accepted);
                            const auto prodWindowed =
                                prodStarted &&
                                recordingWindow(engine, kProdTotal, prodOffset, prodSamples);
                            std::array<float, kProdBlock> prodIn{};
                            prodIn.fill(0.06f);
                            std::array<float, kProdBlock> prodOutL{};
                            std::array<float, kProdBlock> prodOutR{};
                            const std::array<const float*, 1> prodInputs{prodIn.data()};
                            const std::array<float*, 2> prodOutputs{prodOutL.data(),
                                                                    prodOutR.data()};
                            int prodMixed = 0;
                            while (prodWindowed && prodMixed < kProdTotal) {
                                const auto block = std::min(kProdBlock, kProdTotal - prodMixed);
                                engine.mix(prodInputs.data(), 1, prodOutputs.data(), 2, block);
                                prodMixed += block;
                                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                            }
                            const auto preStopStatus = prodCallback.recording().status();
                            juce::String stopError;
                            const auto stopOk = prodCallback.recording().stop(stopError);
                            auto detached = prodCallback.takeFinalizedRecording();
                            const auto processed =
                                detached != nullptr &&
                                engine.processFinalizedRecording(detached.get(), stopError);
                            juce::String finishError;
                            const auto finished =
                                detached != nullptr && detached->finish(processed, finishError);
                            if (finishError.isNotEmpty()) {
                                if (stopError.isNotEmpty()) stopError << " ";
                                stopError << finishError;
                            }
                            if (!engine.stop()) return false;

                            const auto rawFile = prodDir.getChildFile("tracks/0000/raw.wav");
                            const auto processedFile =
                                prodDir.getChildFile("tracks/0000/processed.wav");
                            const auto manifestFile = prodDir.getChildFile("manifest.json");
                            const auto manifestValue =
                                juce::JSON::parse(manifestFile.loadFileAsString());
                            auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(rawFile));
                            auto processedReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                            const auto rawLength =
                                rawReader != nullptr ? rawReader->lengthInSamples : 0;
                            const auto processedLength =
                                processedReader != nullptr ? processedReader->lengthInSamples : 0;
                            const auto completed =
                                manifestValue.isObject() &&
                                manifestValue.getProperty("state", {}).toString() == "completed";

                            diagProductionRawSamples = static_cast<std::uint64_t>(rawLength);
                            diagProductionProcessedSamples =
                                static_cast<std::uint64_t>(processedLength);
                            diagProductionMissing = preStopStatus.processedMissingSamples;
                            diagProductionDropped = preStopStatus.droppedBlocks;

                            productionWriterPassed =
                                prodWindowed && stopOk && processed && finished &&
                                rawFile.existsAsFile() && processedFile.existsAsFile() &&
                                rawLength == kProdTotal && processedLength == kProdTotal &&
                                completed && diagProductionMissing == 0 &&
                                diagProductionDropped == 0;

                            // Verify capture segment ranges match
                            if (productionWriterPassed && manifestValue.isObject()) {
                                const auto manifestTracks = manifestValue.getProperty("tracks", {});
                                if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                    const auto trackObj = manifestTracks[0];
                                    const auto segments =
                                        trackObj.getProperty("captureSegments", {});
                                    if (segments.isArray() && segments.size() == kProdPasses) {
                                        for (int i = 0; i < kProdPasses; ++i) {
                                            const auto seg = segments[i];
                                            const auto rawStart = static_cast<juce::int64>(
                                                seg.getProperty("rawFileStartSample", -1));
                                            const auto rawEnd = static_cast<juce::int64>(
                                                seg.getProperty("rawFileEndSample", -1));
                                            const auto procStart = static_cast<juce::int64>(
                                                seg.getProperty("processedFileStartSample", -1));
                                            const auto procEnd = static_cast<juce::int64>(
                                                seg.getProperty("processedFileEndSample", -1));
                                            if (rawStart != procStart || rawEnd != procEnd) {
                                                productionWriterPassed = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        productionWriterPassed = false;
                                    }
                                } else {
                                    productionWriterPassed = false;
                                }
                            }
                        }

                        // Partial pass: start mid-loop, record partial+full+partial
                        if (!engine.seekToTick(7680)) return false;
                        auto partialConfig = engine.recordingConfiguration();
                        juce::String partialSessionError;
                        auto partialDir = directory.getChildFile("prod-writer-partial");
                        auto partialSession = ArrangeRecordingSession::create(
                            partialDir, partialConfig, partialSessionError);
                        if (partialSession != nullptr) {
                            engine.setRecordingSink(partialSession.get());
                            constexpr int kPartialTotal = 768'000;
                            int partialOffset = 0;
                            int partialSamples = 0;
                            const auto partialStarted =
                                (engine.startRecording(0, error) == RealtimeRequest::accepted);
                            const auto partialWindowed =
                                partialStarted && recordingWindow(engine, kPartialTotal,
                                                                  partialOffset, partialSamples);
                            std::array<float, kProdBlock> partIn{};
                            partIn.fill(0.06f);
                            std::array<float, kProdBlock> partOutL{};
                            std::array<float, kProdBlock> partOutR{};
                            const std::array<const float*, 1> partInputs{partIn.data()};
                            const std::array<float*, 2> partOutputs{partOutL.data(),
                                                                    partOutR.data()};
                            int partMixed = 0;
                            while (partialWindowed && partMixed < kPartialTotal) {
                                const auto block = std::min(kProdBlock, kPartialTotal - partMixed);
                                engine.mix(partInputs.data(), 1, partOutputs.data(), 2, block);
                                partMixed += block;
                                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                            }
                            if (!engine.stopRecording(error)) return false;
                            const auto finalized = finalizeCapturedRecording(engine, error);
                            engine.clearRecordingSink();
                            juce::String finishError;
                            const auto finished = partialSession->finish(finalized, finishError);

                            const auto rawFile = partialDir.getChildFile("tracks/0000/raw.wav");
                            const auto processedFile =
                                partialDir.getChildFile("tracks/0000/processed.wav");
                            const auto manifestFile = partialDir.getChildFile("manifest.json");
                            const auto manifestValue =
                                juce::JSON::parse(manifestFile.loadFileAsString());
                            auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(rawFile));
                            auto processedReader = std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                            const auto rawLength =
                                rawReader != nullptr ? rawReader->lengthInSamples : 0;
                            const auto processedLength =
                                processedReader != nullptr ? processedReader->lengthInSamples : 0;
                            const auto completed =
                                manifestValue.isObject() &&
                                manifestValue.getProperty("state", {}).toString() == "completed";

                            diagProductionPartialRaw = static_cast<std::uint64_t>(rawLength);
                            diagProductionPartialProcessed =
                                static_cast<std::uint64_t>(processedLength);

                            productionWriterPartialPassed =
                                partialWindowed && finalized && finished &&
                                rawFile.existsAsFile() && processedFile.existsAsFile() &&
                                rawLength == kPartialTotal && processedLength == kPartialTotal &&
                                completed;

                            // Verify 3 segments and raw/processed ranges
                            if (productionWriterPartialPassed && manifestValue.isObject()) {
                                const auto manifestTracks = manifestValue.getProperty("tracks", {});
                                if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                    const auto trackObj = manifestTracks[0];
                                    const auto segments =
                                        trackObj.getProperty("captureSegments", {});
                                    if (segments.isArray() && segments.size() == 3) {
                                        for (int i = 0; i < 3; ++i) {
                                            const auto seg = segments[i];
                                            const auto rawStart = static_cast<juce::int64>(
                                                seg.getProperty("rawFileStartSample", -1));
                                            const auto rawEnd = static_cast<juce::int64>(
                                                seg.getProperty("rawFileEndSample", -1));
                                            const auto procStart = static_cast<juce::int64>(
                                                seg.getProperty("processedFileStartSample", -1));
                                            const auto procEnd = static_cast<juce::int64>(
                                                seg.getProperty("processedFileEndSample", -1));
                                            if (rawStart != procStart || rawEnd != procEnd) {
                                                productionWriterPartialPassed = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        productionWriterPartialPassed = false;
                                    }
                                } else {
                                    productionWriterPartialPassed = false;
                                }
                            }
                        }
                    }
                }
                if (loopSnapshotLoaded &&
                    loadTestSnapshot(engine, punchSnapshot, formats, 48000.0, 512, error)) {
                    int punchOffset = 0;
                    int punchSamples = 0;
                    if (!engine.seekToTick(480)) return false;
                    error.clear();
                    const auto punchStarted =
                        (engine.startRecording(0, error) == RealtimeRequest::accepted);
                    punchWindowed = punchStarted &&
                                    recordingWindow(engine, 512, punchOffset, punchSamples) &&
                                    punchOffset == 0 && punchSamples == 512;
                    if (!punchStarted && error.isEmpty())
                        error = "Punch self-test could not start Arrange recording.";
                    if (!engine.stopRecording(error)) return false;
                    if (!engine.stop()) return false;
                    if (!engine.seekToTick(480)) return false;
                    if ((engine.startRecording(0, error) == RealtimeRequest::accepted)) {
                        int immediateOffset = 0;
                        int immediateSamples = 0;
                        std::array<float, 512> immediateOutput{};
                        std::array<float*, 1> immediateChannels{immediateOutput.data()};
                        const auto immediateWindow =
                            recordingWindow(engine, static_cast<int>(immediateOutput.size()),
                                            immediateOffset, immediateSamples);
                        engine.mix(immediateChannels.data(), 1,
                                   static_cast<int>(immediateOutput.size()));
                        const auto immediateStatus = engine.status();
                        immediateRecordStarted =
                            immediateWindow && immediateOffset == 0 && immediateSamples == 512 &&
                            immediateStatus.frame.transportState == TransportState::playing &&
                            immediateStatus.frame.timelineSample == 12'512;
                        if (!engine.stopRecording(error)) return false;
                        if (!engine.stop()) return false;
                        if (!engine.seekToTick(480)) return false;
                    }
                    if ((engine.startRecording(1, error) == RealtimeRequest::accepted)) {
                        int countInOffset = 0;
                        int countInSamples = 0;
                        constexpr int countInBlockSamples = 24'128;
                        const auto countInWindow = recordingWindow(engine, countInBlockSamples,
                                                                   countInOffset, countInSamples);
                        std::vector<float> countInOutput(countInBlockSamples);
                        std::array<float*, 1> countInChannels{countInOutput.data()};
                        engine.mix(countInChannels.data(), 1,
                                   static_cast<int>(countInOutput.size()));
                        engine.mixMetronome(countInChannels.data(), 1,
                                            static_cast<int>(countInOutput.size()));
                        countInAligned = countInWindow && countInOffset == 24'000 &&
                                         countInSamples == 128 &&
                                         engine.status().frame.timelineSample == 12'128;
                        countInAudible =
                            *std::max_element(countInOutput.begin(), countInOutput.end()) > 0.0f;
                        if (!engine.stopRecording(error)) return false;
                    }
                    if (!engine.stop()) return false;
                    if (!engine.seekToTick(480)) return false;
                    if ((engine.startRecording(2, error) == RealtimeRequest::accepted)) {
                        int cancelledOffset = 0;
                        int cancelledSamples = 0;
                        countInCancelled =
                            (engine.cancelRecordingIfCountingIn(error) ==
                             RealtimeRequest::accepted) &&
                            engine.status().frame.recordingPhase == RecordingPhase::idle &&
                            !recordingWindow(engine, 512, cancelledOffset, cancelledSamples) &&
                            cancelledSamples == 0;
                    }
                    if (!engine.play()) return false;
                    if (!engine.seekToTick(0)) return false;
                    std::array<float, 24000> silent{};
                    std::array<float*, 1> silentChannels{silent.data()};
                    engine.mix(silentChannels.data(), 1, static_cast<int>(silent.size()));
                    std::array<float, 240> loopBoundary{};
                    std::array<float*, 1> loopBoundaryChannels{loopBoundary.data()};
                    engine.mix(loopBoundaryChannels.data(), 1,
                               static_cast<int>(loopBoundary.size()));
                    looped = engine.status().frame.timelineSample == 0;
                    std::array<float, 512> clicks{};
                    std::array<float*, 1> clickChannels{clicks.data()};
                    engine.mixMetronome(clickChannels.data(), 1, static_cast<int>(clicks.size()));
                    metronomeMixed = *std::max_element(clicks.begin(), clicks.end()) > 0.0f;
                }
            }
        }

        const auto addCheck = [&checks](const juce::String& name, const bool passed) {
            auto* check = new juce::DynamicObject();
            check->setProperty("name", name);
            check->setProperty("passed", passed);
            checks.add(juce::var(check));
        };
        addCheck("44.1 kHz mono and 48 kHz stereo sources load", sourcesWritten && loaded);
        addCheck("disabled Instrument placeholder does not block the Graph", loaded);
        addCheck("overlapping sources mix through read-ahead and sample-rate correction", mixed);
        addCheck("tick seek resolves against the engine sample clock", seeked);
        addCheck("loop wrap returns to the exact loop start", looped);
        addCheck("punch range limits the recording window", punchWindowed);
        addCheck("stopped Record without count-in starts Transport", immediateRecordStarted);
        addCheck("count-in and Punch capture share the exact callback offset", countInAligned);
        addCheck("count-in click is generated by the Native Clock", countInAudible);
        addCheck("count-in cancellation returns directly to Idle", countInCancelled);
        addCheck("metronome follows the timeline clock", metronomeMixed);
        addCheck("Volume and Pan Automation ramp within an audio block", automationRamped);
        addCheck("Offline Render writes the exact tick selection", offlineRangeRendered);
        addCheck("Offline Render receives audio from the Arrangement Graph", offlineAudioRendered);
        addCheck("Offline Render normalization reaches the target peak", offlineNormalized);
        addCheck("mix edits swap the Graph without reloading Track Devices",
                 graphUpdateReusedDevices);
        addCheck("Parameter and Bypass changes do not alter Plugin Topology",
                 mutablePluginStateKeepsTopology);
        addCheck("recording taps exclude Timeline playback and Track mix gain",
                 recordingTapIsolated);
        addCheck("Timeline loop capture closes three non-overlapping Audio segments",
                 loopCaptureSegments);
        addCheck("Synthetic Plugin loop recording produces aligned Raw/Processed without stopping",
                 syntheticLoopPassed);
        addCheck("Partial Pass recording generates independent Processed segments",
                 partialPassPassed);
        addCheck("Block Size processing respects prepared block size limit", blockSizePassed);
        addCheck("Long Recording (130 passes) matches Raw/Processed without RAM pre-allocation",
                 longRecordingPassed);
        addCheck(
            "Production ThreadedWriter 4小節×3 Pass (AudioRenderPipeline owns transport stop, "
            "capture detachment; lifecycle worker owns offline processing and session finish)",
            productionWriterPassed);
        addCheck("Production ThreadedWriter Partial Pass", productionWriterPartialPassed);
        addCheck("Stopped Transport processes live Instrument MIDI", liveInstrumentWhileStopped);
        addCheck("Timeline panic closes the Instrument runtime", panicClosesRuntime);
        result->setProperty("checks", checks);
        result->setProperty("message", error);
        result->setProperty("partialSegments", diagPartialSegments);
        result->setProperty("partialRaw", diagPartialRaw);
        result->setProperty("partialProcessed", diagPartialProcessed);
        result->setProperty("partialWindowed", diagPartialWindowed);
        result->setProperty("bsRaw", diagBsRaw);
        result->setProperty("bsProcessed", diagBsProcessed);
        result->setProperty("bsWindowed", diagBsWindowed);
        result->setProperty("partialFailIndex", diagPartialFailIndex);
        result->setProperty("partialFailValue", diagPartialFailValue);
        result->setProperty("partialRawAtFail", diagPartialRawAtFail);
        result->setProperty("automationEarlyLeft", automationEarlyLeft);
        result->setProperty("automationEarlyRight", automationEarlyRight);
        result->setProperty("automationLateLeft", automationLateLeft);
        result->setProperty("automationLateRight", automationLateRight);
        result->setProperty("productionRawSamples",
                            static_cast<juce::int64>(diagProductionRawSamples));
        result->setProperty("productionProcessedSamples",
                            static_cast<juce::int64>(diagProductionProcessedSamples));
        result->setProperty("productionMissingSamples",
                            static_cast<juce::int64>(diagProductionMissing));
        result->setProperty("productionDroppedBlocks",
                            static_cast<juce::int64>(diagProductionDropped));
        result->setProperty("productionPartialRaw",
                            static_cast<juce::int64>(diagProductionPartialRaw));
        result->setProperty("productionPartialProcessed",
                            static_cast<juce::int64>(diagProductionPartialProcessed));
        result->setProperty(
            "passed", sourcesWritten && loaded && mixed && seeked && looped && punchWindowed &&
                          immediateRecordStarted && countInAligned && countInAudible &&
                          countInCancelled && metronomeMixed && automationRamped &&
                          offlineRangeRendered && offlineAudioRendered && offlineNormalized &&
                          graphUpdateReusedDevices && mutablePluginStateKeepsTopology &&
                          recordingTapIsolated && loopCaptureSegments && syntheticLoopPassed &&
                          partialPassPassed && blockSizePassed && longRecordingPassed &&
                          productionWriterPassed && productionWriterPartialPassed &&
                          liveInstrumentWhileStopped && panicClosesRuntime);
        mono.deleteFile();
        stereo.deleteFile();
        directory.getChildFile("offline-selection.wav").deleteFile();
        directory.getChildFile("offline-normalized.wav").deleteFile();
        return juce::var(result);
    }
};
}  // namespace riffra
