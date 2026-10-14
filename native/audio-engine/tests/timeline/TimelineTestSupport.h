#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <memory>
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
    snapshot.graph.timebase = {960, {{0, 120.0}}, {{0, 4, 4}}};
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
    void writeMidiTrack(const juce::String&, const juce::String& sourceDeviceId,
                        const juce::MidiMessage&, std::uint64_t) noexcept override {
        receivedMidiSourceId = sourceDeviceId;
    }
    void writeMidiTrack(const juce::String&, const std::uint16_t sourceIndex,
                        const juce::MidiMessage&, std::uint64_t) noexcept override {
        receivedMidiSourceIndex = sourceIndex;
    }
    void setMidiSourceIds(const std::vector<juce::String>& sourceIds) override {
        if (receivedMidiSourceIndex < sourceIds.size())
            receivedMidiSourceId = sourceIds[receivedMidiSourceIndex];
    }
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
    juce::String receivedMidiSourceId;
    std::uint16_t receivedMidiSourceIndex = MidiSourceRegistry::kUnregistered;
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
    void writeMidiTrack(const juce::String&, std::uint16_t, const juce::MidiMessage&,
                        std::uint64_t) noexcept override {}
    void setMidiSourceIds(const std::vector<juce::String>&) override {}
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

bool finalizeCapturedRecording(TimelineEngine& engine, juce::String& error);

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
        track.monitorInput = primary && monitorFirstTrack;
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
    static ArrangementCaptureSink* recordingSink(TimelineEngine& engine) {
        return engine.controlRecordingSink;
    }
    static bool setOfflineCompensation(OfflineRenderer& renderer, const juce::String& trackId,
                                       const std::int64_t samples) {
        return setPlaybackCompensationForTest(*renderer.engine, trackId, samples);
    }

    static bool installOfflineEffect(OfflineRenderer& renderer, const juce::String& trackId,
                                     std::unique_ptr<juce::AudioProcessor> processor,
                                     juce::String& error) {
        return installTrackChainDevice(*renderer.engine, trackId, "effect:test",
                                       std::move(processor), 48'000.0, 128, error);
    }

    static void setOfflineWorkerCount(OfflineRenderer& renderer, const int workerCount) {
        renderer.engine->pool = std::make_unique<TrackProcessingPool>(workerCount);
    }

    static bool addChainDevice(PluginChain& chain, const juce::String& id,
                               std::unique_ptr<juce::AudioProcessor> processor,
                               const double sampleRate, const int blockSize, juce::String& error) {
        auto rack = PluginRackTestPeer::install(std::move(processor), sampleRate, blockSize, error);
        if (rack == nullptr) return false;
        chain.devices.push_back(PluginChain::Device{id, std::move(rack)});
        chain.prepareBuffers(blockSize);
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
        for (auto& route : graph->mixer.trackRoutes)
            if (route->track == track.runtime.get())
                route->delay.prepare(static_cast<int>(samples));
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

    static bool hasRecordingSink(TimelineEngine& engine) {
        return engine.controlRecordingSink != nullptr;
    }

    static std::uint64_t nextCommandSequence(TimelineEngine& engine) {
        const std::lock_guard lock(engine.ownerMutex);
        return engine.nextCommandSequence;
    }

    static std::size_t realtimeCommandCapacity() noexcept {
        return TimelineEngine::kRealtimeCommandCapacity;
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
        second.graph.automation.push_back({AutomationTargetKind::trackVolume,
                                           secondTrack.id,
                                           {},
                                           AutomationInterpolationSpec::linear,
                                           {{480, -3.0}}});

        // Act
        if (!engine.loadSnapshot(second, formats, 48'000.0, 32, error, false) ||
            !engine.preparedOwnerReusesRuntimeDevices(
                EffectOwnerSpec{EffectOwnerKindSpec::track, "track:state"}) ||
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
        if (!engine.setDeviceParameter(
                EffectOwnerSpec{EffectOwnerKindSpec::track, "track:editor-instrument"},
                "instrument:editor", 0, 0.75f, error))
            return false;
        if (!engine.play()) return false;
        std::array<float, 32> left{};
        std::array<float, 32> right{};
        std::array<float*, 2> outputs{left.data(), right.data()};
        engine.mix(outputs.data(), 2, static_cast<int>(left.size()));

        // Assert
        const auto state = rackPointer->persistedState(error);
        if (!state.has_value() || state->parameterValues.empty() ||
            std::abs(state->parameterValues[0] - 0.75f) > 0.0001f)
            return false;

        PluginRackTestPeer::addParameterAndPublishCount(
            *rackPointer, new juce::AudioParameterFloat("added", "Added", 0.0f, 1.0f, 0.0f));
        const auto owner = EffectOwnerSpec{EffectOwnerKindSpec::track, "track:editor-instrument"};
        if (engine.setDeviceParameter(owner, "instrument:editor", 700, 1.0f, error) ||
            error != "Plugin parameter change queue cannot handle this index.")
            return false;
        const auto unchanged = rackPointer->persistedState(error);
        return unchanged.has_value() && unchanged->parameterValues.size() == 701u &&
               std::abs(unchanged->parameterValues[0] - 0.75f) <= 0.0001f &&
               unchanged->parameterValues[700] == 0.0f;
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
        const auto changed = engine.setDevicePersistedState(
            EffectOwnerSpec{EffectOwnerKindSpec::track, "track:plugin-state"},
            "instrument:plugin-state", PluginStateSpec{std::nullopt, {0.75f}, false}, error);

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
        const auto changed = engine.setDeviceProgram(
            EffectOwnerSpec{EffectOwnerKindSpec::track, "track:plugin-program"},
            "instrument:plugin-program", 1, error);

        // Assert
        return changed && trace.currentProgram == 1;
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
            for (auto& route : graph->mixer.trackRoutes) route->delay.prepare(4);
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
            !engine.preparedOwnerReusesRuntimeDevices(
                EffectOwnerSpec{EffectOwnerKindSpec::track, "track:audio-device"}) ||
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
};

namespace {

bool finalizeCapturedRecording(TimelineEngine& engine, juce::String& error) {
    if (!engine.stopRecording(error) || !engine.finalizeRecording(error)) return false;
    if (!engine.stop()) return false;
    return engine.processFinalizedRecording(TimelineEngineTestPeer::recordingSink(engine), error);
}

}  // namespace

}  // namespace riffra
