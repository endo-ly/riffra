#include "TimelineEngine.h"
#include "ArrangeRecordingSession.h"
#include "ArrangementGraph.h"
#include "OfflineRenderer.h"
#include "SafetyAudioCallback.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <fstream>
#include <limits>
#include <thread>

namespace riffra {

namespace {
constexpr int kReadAheadSamples = 32768;

juce::String pluginTopologySignature(const juce::var& values) {
    juce::Array<juce::var> topology;
    const auto append = [&topology](const juce::var& value) {
        if (!value.isObject())
            return;
        auto* device = new juce::DynamicObject();
        device->setProperty("id", value.getProperty("id", {}));
        device->setProperty("kind", value.getProperty("kind", {}));
        device->setProperty("path", value.getProperty("path", {}));
        device->setProperty(
            "disabledPlaceholder",
            value.getProperty("disabledPlaceholder", false));
        topology.add(juce::var(device));
    };
    if (values.isArray()) {
        for (const auto& value : *values.getArray())
            append(value);
    } else if (values.isObject()) {
        append(values);
    }
    return juce::JSON::toString(juce::var(topology), false);
}

bool requiredNumber(const juce::var& object, const juce::Identifier& name, double& value) {
    const auto property = object.getProperty(name, {});
    if (!property.isInt() && !property.isInt64() && !property.isDouble()) return false;
    value = static_cast<double>(property);
    return std::isfinite(value);
}

bool writePcmWave(
    const juce::File& file,
    const std::uint32_t sampleRate,
    const std::uint16_t channels,
    const std::uint32_t frames,
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

    bool beginAudioTrackCapture(
        const juce::String& trackId,
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
    void writeAudioTrack(
        const juce::String& trackId,
        const float* raw,
        const int rawSampleCount,
        const float* const* processed,
        const int processedSampleCount) noexcept override {
        receivedTrack = trackId;
        receivedSamples = rawSampleCount;
        currentRawSamples += std::max(0, rawSampleCount);
        totalRawSamples += std::max(0, rawSampleCount);
        totalProcessedSamples += std::max(0, processedSampleCount);
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
        isolated = raw != nullptr && processed != nullptr
            && processed[0] != nullptr && processed[1] != nullptr
            && rawSampleCount == processedSampleCount;
        for (int sample = 0; isolated && sample < rawSampleCount; ++sample)
            isolated = std::abs(raw[sample] - 0.05f) < 0.0001f
                && std::abs(processed[0][sample] - 0.05f) < 0.0001f
                && std::abs(processed[1][sample] - 0.05f) < 0.0001f;
    }
    bool writeProcessedAudioTrackOffline(
        const juce::String&,
        const float* const* processed,
        const int sampleCount,
        int) noexcept override {
        if (processed != nullptr && sampleCount > 0
            && processed[0] != nullptr && processed[1] != nullptr) {
            totalProcessedSamples += sampleCount;
            maxOfflineProcessedWriteSize =
                std::max(maxOfflineProcessedWriteSize, sampleCount);
            ++offlineProcessedWriteCalls;
        }
        return true;
    }

    bool endAudioTrackCapture(
        const juce::String&,
        const std::uint64_t audioClockEndSample,
        const std::uint64_t timelineEndSample) noexcept override {
        if (endCount < static_cast<int>(endAudioSamples.size())) {
            endAudioSamples[static_cast<std::size_t>(endCount)] = audioClockEndSample;
            endTimelineSamples[static_cast<std::size_t>(endCount)] = timelineEndSample;
            segmentRawSamples[static_cast<std::size_t>(endCount)] = currentRawSamples;
        }
        segmentRanges.emplace_back(
            segmentStartSample, rawBuffer.size());
        ++endCount;
        return true;
    }
    bool completeAudioTrackTail(const juce::String&) noexcept override { return true; }

    void markLoopBoundary(const std::uint64_t audioClockSample) noexcept override {
        if (loopBoundaryCount < static_cast<int>(loopBoundarySamples.size()))
            loopBoundarySamples[static_cast<std::size_t>(loopBoundaryCount)] = audioClockSample;
        ++loopBoundaryCount;
    }
    void writeMidiTrack(
        const juce::String&,
        const juce::String&,
        const juce::MidiMessage&,
        std::uint64_t) noexcept override {}
    void setCaptureRange(
        std::uint64_t,
        std::uint64_t,
        std::uint64_t,
        std::uint64_t) noexcept override {}

    juce::File prepareRawForReading(const juce::String&) noexcept override {
        if (testDirectory == juce::File {} || rawBuffer.empty())
            return {};
        const auto file = testDirectory.getChildFile("capture-isolation-raw.wav");
        file.deleteFile();
        std::unique_ptr<juce::OutputStream> os(file.createOutputStream());
        if (os == nullptr)
            return {};
        juce::WavAudioFormat wav;
        auto writer = wav.createWriterFor(
            os,
            juce::AudioFormatWriterOptions()
                .withSampleRate(48000.0)
                .withNumChannels(1)
                .withBitsPerSample(32)
                .withSampleFormat(
                    juce::AudioFormatWriterOptions::SampleFormat::floatingPoint));
        if (writer == nullptr)
            return {};
        const auto numSamples = static_cast<int>(rawBuffer.size());
        juce::AudioBuffer<float> writeBuffer(1, numSamples);
        writeBuffer.copyFrom(0, 0, rawBuffer.data(), numSamples);
        writer->writeFromAudioSampleBuffer(writeBuffer, 0, numSamples);
        writer->flush();
        return file;
    }

    std::vector<std::pair<std::uint64_t, std::uint64_t>>
    getRawSegmentRanges(const juce::String&) noexcept override {
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
    std::array<std::uint64_t, 8> beginAudioSamples {};
    std::array<std::uint64_t, 8> beginTimelineSamples {};
    std::array<std::uint64_t, 8> endAudioSamples {};
    std::array<std::uint64_t, 8> endTimelineSamples {};
    std::array<int, 8> segmentRawSamples {};
    std::array<std::uint64_t, 8> loopBoundarySamples {};

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

    bool beginAudioTrackCapture(
        const juce::String&,
        std::uint64_t,
        std::uint64_t) noexcept override {
        ++segmentCount;
        segmentStartSample = rawBuffer.size();
        return true;
    }
    void writeAudioTrack(
        const juce::String&,
        const float* raw,
        int rawSampleCount,
        const float* const* processed,
        int processedSampleCount) noexcept override {
        if (raw != nullptr && rawSampleCount > 0)
            rawBuffer.insert(rawBuffer.end(), raw, raw + rawSampleCount);
        if (processed != nullptr && processedSampleCount > 0
            && processed[0] != nullptr && processed[1] != nullptr) {
            processedLeft.insert(
                processedLeft.end(), processed[0], processed[0] + processedSampleCount);
            processedRight.insert(
                processedRight.end(), processed[1], processed[1] + processedSampleCount);
            maxProcessedWriteSize = std::max(maxProcessedWriteSize, processedSampleCount);
        }
        totalRaw += std::max(0, rawSampleCount);
        totalProcessed += std::max(0, processedSampleCount);
    }
    bool writeProcessedAudioTrackOffline(
        const juce::String&,
        const float* const* processed,
        const int sampleCount,
        int) noexcept override {
        if (processed != nullptr && sampleCount > 0
            && processed[0] != nullptr && processed[1] != nullptr) {
            processedLeft.insert(
                processedLeft.end(), processed[0], processed[0] + sampleCount);
            processedRight.insert(
                processedRight.end(), processed[1], processed[1] + sampleCount);
            maxOfflineProcessedWriteSize =
                std::max(maxOfflineProcessedWriteSize, sampleCount);
            ++offlineProcessedWriteCalls;
        }
        totalProcessed += std::max(0, sampleCount);
        return true;
    }
    bool endAudioTrackCapture(
        const juce::String&,
        std::uint64_t,
        std::uint64_t) noexcept override {
        segmentRanges.emplace_back(segmentStartSample, rawBuffer.size());
        return true;
    }
    bool completeAudioTrackTail(const juce::String&) noexcept override { return true; }
    void markLoopBoundary(std::uint64_t) noexcept override { ++boundaryCount; }
    void writeMidiTrack(
        const juce::String&,
        const juce::String&,
        const juce::MidiMessage&,
        std::uint64_t) noexcept override {}
    void setCaptureRange(
        std::uint64_t,
        std::uint64_t,
        std::uint64_t,
        std::uint64_t) noexcept override {}

    juce::File prepareRawForReading(const juce::String&) noexcept override {
        if (rawBuffer.empty())
            return {};
        const auto file = testDirectory.getChildFile(fileName + "-raw.wav");
        file.deleteFile();
        std::unique_ptr<juce::OutputStream> os(file.createOutputStream());
        if (os == nullptr)
            return {};
        juce::WavAudioFormat wav;
        auto writer = wav.createWriterFor(
            os,
            juce::AudioFormatWriterOptions()
                .withSampleRate(48000.0)
                .withNumChannels(2)
                .withBitsPerSample(32)
                .withSampleFormat(
                    juce::AudioFormatWriterOptions::SampleFormat::floatingPoint));
        if (writer == nullptr)
            return {};
        const auto numSamples = static_cast<int>(rawBuffer.size());
        juce::AudioBuffer<float> writeBuffer(2, numSamples);
        writeBuffer.copyFrom(0, 0, rawBuffer.data(), numSamples);
        writeBuffer.copyFrom(1, 0, rawBuffer.data(), numSamples);
        writer->writeFromAudioSampleBuffer(writeBuffer, 0, numSamples);
        writer->flush();
        return file;
    }

    std::vector<std::pair<std::uint64_t, std::uint64_t>>
    getRawSegmentRanges(const juce::String&) noexcept override {
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
} // namespace

TimelineEngine::TimelineEngine(const bool offline)
    : offlineMode(offline),
      recordingCapture(std::make_unique<RecordingCaptureRuntime>()) {
    if (!offlineMode)
        readAheadThread.startThread();
}

TimelineEngine::~TimelineEngine() {
    stop();
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
        pendingTimeline.reset();
    }
    if (readAheadThread.isThreadRunning())
        readAheadThread.stopThread(3000);
}

bool TimelineEngine::loadSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    juce::String& error,
    const bool commitImmediately) {
    std::unique_ptr<PreparedTimeline> prepared;
    bool monitorLiveInputState = false;
    bool armedInstrumentTrackState = false;
    if (!prepareSnapshot(
            snapshot,
            formats,
            outputSampleRate,
            maximumBlockSize,
            prepared,
            monitorLiveInputState,
            armedInstrumentTrackState,
            error))
        return false;

    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        pendingTimeline = std::move(prepared);
        pendingMonitorLiveInput = monitorLiveInputState;
        pendingArmedInstrumentTrack = armedInstrumentTrackState;
    }
    if (!commitImmediately)
        return true;
    return commitPreparedSnapshot(error);
}

bool TimelineEngine::prepareSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    std::unique_ptr<PreparedTimeline>& prepared,
    bool& monitorLiveInputState,
    bool& armedInstrumentTrackState,
    juce::String& error) {
    prepared.reset();
    monitorLiveInputState = false;
    armedInstrumentTrackState = false;
    if (!snapshot.isObject() || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }
    prepared = std::make_unique<PreparedTimeline>();
    prepared->revision = static_cast<std::uint64_t>(
        static_cast<juce::int64>(snapshot.getProperty("revision", -1)));
    const auto unavailableClipIds = snapshot.getProperty("unavailableClipIds", {});
    if (unavailableClipIds.isArray())
        prepared->unavailableClipIds = *unavailableClipIds.getArray();
    const auto missingDeviceIds = snapshot.getProperty("missingDeviceIds", {});
    if (missingDeviceIds.isArray())
        prepared->missingDeviceIds = *missingDeviceIds.getArray();
    const auto timebase = snapshot.getProperty("timebase", {});
    double ppq = 0.0;
    if (!timebase.isObject() || !requiredNumber(timebase, "ppq", ppq) ||
        !requiredNumber(timebase, "bpm", prepared->timebase.bpm) || ppq != 960.0 ||
        prepared->timebase.bpm < 20.0 || prepared->timebase.bpm > 400.0) {
        error = "Timeline snapshot has an invalid timebase.";
        return false;
    }
    prepared->timebase.ppq = static_cast<std::uint32_t>(ppq);
    prepared->outputSampleRate = outputSampleRate;
    prepared->preparedBlockSize = maximumBlockSize;
    const auto denominator = static_cast<int>(timebase.getProperty("timeSignatureDenominator", 4));
    const auto numerator = static_cast<int>(timebase.getProperty("timeSignatureNumerator", 4));
    if (denominator <= 0 || numerator <= 0) {
        error = "Timeline snapshot has an invalid time signature.";
        return false;
    }
    const auto beatTicks = static_cast<double>(prepared->timebase.ppq) * 4.0 / denominator;
    prepared->beatSamples = prepared->timebase.tickToSample(
        static_cast<std::uint64_t>(std::llround(beatTicks)),
        outputSampleRate);
    prepared->beatsPerBar = numerator;
    prepared->metronomeEnabled = static_cast<bool>(snapshot.getProperty("metronomeEnabled", false));

    const auto loopRange = snapshot.getProperty("loopRange", {});
    if (loopRange.isObject()) {
        prepared->loopEnabled = static_cast<bool>(loopRange.getProperty("enabled", false));
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("endTick", 0)));
        prepared->loopStartSample = prepared->timebase.tickToSample(
            startTick, outputSampleRate);
        prepared->loopEndSample = prepared->timebase.tickToSample(
            endTick, outputSampleRate);
        if (prepared->loopEnabled && prepared->loopEndSample <= prepared->loopStartSample) {
            error = "Timeline loop range must have a positive duration.";
            return false;
        }
    }

    const auto punchRange = snapshot.getProperty("punchRange", {});
    if (punchRange.isObject()) {
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(punchRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(punchRange.getProperty("endTick", 0)));
        prepared->punchStartSample = prepared->timebase.tickToSample(
            startTick, outputSampleRate);
        prepared->punchEndSample = prepared->timebase.tickToSample(
            endTick, outputSampleRate);
        if (prepared->punchEndSample <= prepared->punchStartSample) {
            error = "Timeline punch range must have a positive duration.";
            return false;
        }
        prepared->punchEnabled = true;
    }

    const auto tracks = snapshot.getProperty("tracks", {});
    if (!tracks.isArray()) {
        error = "Timeline snapshot tracks must be an array.";
        return false;
    }
    std::int64_t maximumPluginDelay = 0;
    for (const auto& trackValue : *tracks.getArray()) {
        if (!trackValue.isObject()) {
            error = "Timeline track must be an object.";
            return false;
        }
        auto track = std::make_unique<Track>();
        track->id = trackValue.getProperty("id", {}).toString();
        track->outputSampleRate = outputSampleRate;
        track->instrument = trackValue.getProperty("kind", {}).toString() == "instrument";
        track->armed = static_cast<bool>(trackValue.getProperty("armed", false));
        const auto midiInput = trackValue.getProperty("midiInput", {});
        if (midiInput.isObject()) {
            track->midiDeviceId = midiInput.getProperty("deviceId", {}).toString();
            track->midiChannel = static_cast<int>(midiInput.getProperty("channel", 0));
        }
        armedInstrumentTrackState |= track->instrument && track->armed;
        if (track->id.isEmpty()) {
            error = "Timeline track requires an id.";
            return false;
        }
        track->gainDb = juce::jlimit(
            -90.0f, 24.0f, static_cast<float>(trackValue.getProperty("gainDb", 0.0)));
        track->pan = juce::jlimit(
            -1.0f, 1.0f, static_cast<float>(trackValue.getProperty("pan", 0.0)));
        track->muted = static_cast<bool>(trackValue.getProperty("muted", false));
        track->solo = static_cast<bool>(trackValue.getProperty("solo", false));
        const auto monitoring = trackValue.getProperty("monitoring", {}).toString();
        track->monitorInput = ArrangementGraph::shouldMonitorAudioInput(
            monitoring, track->armed, track->instrument);
        if (track->monitorInput)
            monitorLiveInputState = true;
        const auto audioInput = trackValue.getProperty("audioInput", {});
        if (audioInput.isObject())
            track->audioInputChannel =
                static_cast<int>(audioInput.getProperty("channelIndex", -1));

        const auto automation = trackValue.getProperty(
            "automation", juce::var(juce::Array<juce::var> {}));
        if (!automation.isArray()) {
            error = "Timeline track automation must be an array.";
            return false;
        }
        for (const auto& lane : *automation.getArray()) {
            if (!lane.isObject()) {
                error = "Timeline Automation Lane must be an object.";
                return false;
            }
            const auto parameter = lane.getProperty("parameter", {}).toString();
            const auto pointValues = lane.getProperty("points", {});
            if ((parameter != "volume" && parameter != "pan") || !pointValues.isArray()) {
                error = "Timeline Automation Lane has an invalid parameter or point list.";
                return false;
            }
            auto& destination = parameter == "volume"
                ? track->volumeAutomation
                : track->panAutomation;
            for (const auto& pointValue : *pointValues.getArray()) {
                if (!pointValue.isObject()) {
                    error = "Timeline Automation Point must be an object.";
                    return false;
                }
                const auto tick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    pointValue.getProperty("tick", 0)));
                const auto value = static_cast<float>(pointValue.getProperty("value", 0.0));
                if (!std::isfinite(value)) {
                    error = "Timeline Automation Point must have a finite value.";
                    return false;
                }
                destination.push_back({
                    prepared->timebase.tickToSample(tick, outputSampleRate),
                    parameter == "volume"
                        ? juce::jlimit(-90.0f, 24.0f, value)
                        : juce::jlimit(-1.0f, 1.0f, value),
                });
            }
        }

        const auto rack = trackValue.getProperty("rack", {});
        const auto instrument = trackValue.getProperty("instrument", {});
        const auto devices = rack.isObject()
            ? rack.getProperty("devices", {})
            : juce::var(juce::Array<juce::var> {});
        track->effectTopologySignature = pluginTopologySignature(devices);
        track->instrumentTopologySignature = pluginTopologySignature(instrument);
        track->effectState = devices;
        track->instrumentState = instrument;
        track->instrumentDeviceId = instrument.isObject()
            ? instrument.getProperty("id", {}).toString()
            : juce::String();
        {
            const juce::SpinLock::ScopedLockType lock(timelineLock);
            if (timeline != nullptr) {
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&track](const auto& item) { return item->id == track->id; });
                if (existing != timeline->tracks.end()
                    && (*existing)->effectTopologySignature
                        == track->effectTopologySignature
                    && (*existing)->instrumentTopologySignature
                        == track->instrumentTopologySignature) {
                    track->reuseRuntimeDevices = true;
                    track->effectStateChanged =
                        juce::JSON::toString((*existing)->effectState, false)
                        != juce::JSON::toString(track->effectState, false);
                    track->instrumentStateChanged =
                        juce::JSON::toString((*existing)->instrumentState, false)
                        != juce::JSON::toString(track->instrumentState, false);
                    track->pluginDelaySamples = (*existing)->pluginDelaySamples;
                    track->pluginTailSamples = (*existing)->pluginTailSamples;
                }
            }
        }
        if (rack.isObject()) {
            if (!track->reuseRuntimeDevices
                && !track->effectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
            if (!track->reuseRuntimeDevices && !track->instrument &&
                !track->liveEffectChain.load(devices, outputSampleRate, maximumBlockSize, error))
                return false;
            if (!track->reuseRuntimeDevices && !track->instrument &&
                !track->recordingCapture.effectChain.load(
                    devices, outputSampleRate, maximumBlockSize, error))
                return false;
        }
        if (instrument.isObject()
            && !static_cast<bool>(
                instrument.getProperty("disabledPlaceholder", false))
            && !track->reuseRuntimeDevices) {
            const auto path = instrument.getProperty("path", {}).toString();
            track->instrumentRack = std::make_unique<PluginRack>();
            if (const auto loadError =
                    track->instrumentRack->load(path, outputSampleRate, maximumBlockSize)) {
                error = "Track Instrument could not be loaded: " + loadError->message;
                return false;
            }
            if (!track->instrumentRack->applyPersistedState(instrument, error))
                return false;
        }
        if (!track->reuseRuntimeDevices) {
            track->pluginDelaySamples = track->effectChain.latencySamples() +
                (track->instrumentRack != nullptr ? track->instrumentRack->latencySamples() : 0);
            track->pluginTailSamples = track->effectChain.tailSamples()
                + (track->instrumentRack != nullptr ? track->instrumentRack->tailSamples() : 0);
        }
        maximumPluginDelay = std::max(maximumPluginDelay, track->pluginDelaySamples);

        const auto clips = trackValue.getProperty("audioClips", {});
        if (!clips.isArray()) {
            error = "Timeline track audioClips must be an array.";
            return false;
        }
        for (const auto& value : *clips.getArray()) {
            if (!value.isObject()) {
                error = "Timeline clip must be an object.";
                return false;
            }
            const auto path = value.getProperty("path", {}).toString();
            auto reader = std::unique_ptr<juce::AudioFormatReader>(
                formats.createReaderFor(juce::File(path)));
            if (reader == nullptr || reader->lengthInSamples <= 0 || reader->sampleRate <= 0.0) {
                error = "Timeline source could not be opened: " + path;
                return false;
            }
            auto clip = std::make_unique<Clip>();
            clip->id = value.getProperty("clipId", {}).toString();
            const auto declaredSourceRate = static_cast<double>(
                value.getProperty("sourceSampleRate", 0.0));
            clip->sourceSampleRate = reader->sampleRate;
            clip->sourceStartFrame = static_cast<juce::int64>(
                value.getProperty("sourceStartFrame", 0));
            clip->sourceEndFrame = static_cast<juce::int64>(
                value.getProperty("sourceEndFrame", 0));
            const auto durationFrames = static_cast<juce::int64>(
                value.getProperty("durationFrames", 0));
            const auto durationRate = static_cast<double>(
                value.getProperty("durationSampleRate", 0.0));
            if (clip->id.isEmpty() || declaredSourceRate <= 0.0 ||
                std::abs(declaredSourceRate - reader->sampleRate) > 0.5 ||
                clip->sourceStartFrame < 0 ||
                clip->sourceEndFrame <= clip->sourceStartFrame ||
                clip->sourceEndFrame > reader->lengthInSamples || durationFrames <= 0 ||
                durationRate <= 0.0) {
                error = "Timeline clip has an invalid frame range: " + clip->id;
                return false;
            }
            const auto startTick = static_cast<std::uint64_t>(
                static_cast<juce::int64>(value.getProperty("startTick", 0)));
            clip->startSample = prepared->timebase.tickToSample(
                startTick, outputSampleRate);
            clip->durationSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(durationFrames) * outputSampleRate / durationRate));
            const auto fadeInFrames = static_cast<juce::int64>(
                value.getProperty("fadeInFrames", 0));
            const auto fadeOutFrames = static_cast<juce::int64>(
                value.getProperty("fadeOutFrames", 0));
            clip->fadeInSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(fadeInFrames) * outputSampleRate / durationRate));
            clip->fadeOutSamples = static_cast<std::int64_t>(std::llround(
                static_cast<double>(fadeOutFrames) * outputSampleRate / durationRate));
            clip->gain = juce::Decibels::decibelsToGain(
                static_cast<float>(value.getProperty("gainDb", 0.0)));
            clip->pan = juce::jlimit(
                -1.0f, 1.0f, static_cast<float>(value.getProperty("pan", 0.0)));
            clip->loop = static_cast<bool>(value.getProperty("loopEnabled", false));
            clip->muted = static_cast<bool>(value.getProperty("muted", false));
            clip->readerSource = std::make_unique<juce::AudioFormatReaderSource>(reader.release(), true);
            clip->transport.setSource(
                clip->readerSource.get(),
                offlineMode ? 0 : kReadAheadSamples,
                offlineMode ? nullptr : &readAheadThread,
                clip->sourceSampleRate,
                2);
            clip->transport.prepareToPlay(maximumBlockSize, outputSampleRate);
            clip->transport.start();
            clip->scratch.setSize(2, maximumBlockSize, false, true, false);
            track->clips.push_back(std::move(clip));
        }
        const auto midiClips = trackValue.getProperty("midiClips", {});
        if (!midiClips.isArray()) {
            error = "Timeline track midiClips must be an array.";
            return false;
        }
        for (const auto& value : *midiClips.getArray()) {
            if (!value.isObject()) {
                error = "Timeline MIDI clip must be an object.";
                return false;
            }
            MidiClip midiClip;
            midiClip.startTick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                value.getProperty("startTick", 0)));
            midiClip.durationTicks = static_cast<std::uint64_t>(static_cast<juce::int64>(
                value.getProperty("durationTicks", 0)));
            midiClip.loop = static_cast<bool>(value.getProperty("loopEnabled", false));
            midiClip.muted = static_cast<bool>(value.getProperty("muted", false));
            if (midiClip.durationTicks == 0) {
                error = "Timeline MIDI clip must have a positive duration.";
                return false;
            }
            const auto notes = value.getProperty("notes", {});
            if (!notes.isArray()) {
                error = "Timeline MIDI clip notes must be an array.";
                return false;
            }
            for (const auto& noteValue : *notes.getArray()) {
                if (!noteValue.isObject()) {
                    error = "Timeline MIDI note must be an object.";
                    return false;
                }
                MidiNote note;
                note.startTick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    noteValue.getProperty("startTick", 0)));
                note.durationTicks = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    noteValue.getProperty("durationTicks", 0)));
                note.note = juce::jlimit(0, 127, static_cast<int>(noteValue.getProperty("note", -1)));
                note.velocity = juce::jlimit(
                    1, 127, static_cast<int>(noteValue.getProperty("velocity", 0)));
                note.channel = juce::jlimit(
                    1, 16, static_cast<int>(noteValue.getProperty("channel", 0)));
                if (note.durationTicks == 0 || note.startTick >= midiClip.durationTicks) {
                    error = "Timeline MIDI note has an invalid musical range.";
                    return false;
                }
                midiClip.notes.push_back(note);
            }
            const auto events = value.getProperty("events", {});
            if (!events.isArray()) {
                error = "Timeline MIDI events must be an array.";
                return false;
            }
            for (const auto& eventValue : *events.getArray()) {
                if (!eventValue.isObject()) {
                    error = "Timeline MIDI event must be an object.";
                    return false;
                }
                MidiEvent event;
                event.kind = eventValue.getProperty("kind", {}).toString();
                event.tick = static_cast<std::uint64_t>(static_cast<juce::int64>(
                    eventValue.getProperty("tick", 0)));
                event.channel = juce::jlimit(
                    1, 16, static_cast<int>(eventValue.getProperty("channel", 0)));
                event.data1 = juce::jlimit(0, 127, static_cast<int>(
                    eventValue.getProperty("data1", 0)));
                event.data2 = juce::jlimit(0, 127, static_cast<int>(
                    eventValue.getProperty("data2", 0)));
                if (event.tick >= midiClip.durationTicks ||
                    (event.kind != "controlChange" && event.kind != "pitchBend" &&
                     event.kind != "channelPressure")) {
                    error = "Timeline MIDI event has an invalid type or musical position.";
                    return false;
                }
                midiClip.events.push_back(event);
            }
            track->midiClips.push_back(std::move(midiClip));
        }
        track->mixBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->liveInputBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->liveProcessedBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->recordingCapture.processedBuffer.setSize(
            2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }
    for (auto& track : prepared->tracks) {
        track->compensationDelaySamples = ArrangementGraph::compensationDelay(
            maximumPluginDelay, track->pluginDelaySamples);
        track->delayBuffer.setSize(
            2, static_cast<int>(track->compensationDelaySamples + maximumBlockSize + 1),
            false, true, false);
        track->delayBuffer.clear();
    }

    return true;
}

bool TimelineEngine::commitPreparedSnapshot(juce::String& error) noexcept {
    std::unique_ptr<PreparedTimeline> candidate;
    std::unique_ptr<PreparedTimeline> retiredTimeline;
    bool hasExistingTimeline = false;
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        if (pendingTimeline == nullptr) {
            error = "No prepared Timeline snapshot is available.";
            return false;
        }
        candidate = std::move(pendingTimeline);
        hasExistingTimeline = timeline != nullptr;
        if (timeline != nullptr) {
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices)
                    continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id
                            && item->effectTopologySignature
                                == candidateTrack->effectTopologySignature
                            && item->instrumentTopologySignature
                                == candidateTrack->instrumentTopologySignature;
                    });
                if (existing == timeline->tracks.end()) {
                    error = "Timeline device runtime changed while the snapshot was prepared.";
                    pendingTimeline = std::move(candidate);
                    return false;
                }
                candidateTrack->effectChain = std::move((*existing)->effectChain);
                candidateTrack->liveEffectChain = std::move((*existing)->liveEffectChain);
                candidateTrack->recordingCapture.effectChain =
                    std::move((*existing)->recordingCapture.effectChain);
                candidateTrack->instrumentRack = std::move((*existing)->instrumentRack);
            }
        }

        // Reused plugins are stateful objects shared with the active timeline.
        // Apply the pending state before publishing the candidate so a failed
        // state update cannot leave a new, partially configured timeline live.
        const auto applyPendingState = [&]() noexcept {
            for (auto& track : candidate->tracks) {
                if (!track->reuseRuntimeDevices)
                    continue;
                if (track->effectStateChanged
                    && !track->effectChain.applyState(track->effectState, error))
                    return false;
                if (track->effectStateChanged
                    && !track->instrument
                    && !track->liveEffectChain.applyState(track->effectState, error))
                    return false;
                if (track->effectStateChanged
                    && !track->instrument
                    && !track->recordingCapture.effectChain.applyState(
                        track->effectState, error))
                    return false;
                if (track->instrumentStateChanged
                    && track->instrumentRack != nullptr
                    && !track->instrumentRack->applyPersistedState(
                        track->instrumentState, error))
                    return false;
            }
            return true;
        };
        const auto restoreRuntimeDevices = [&]() noexcept {
            if (timeline == nullptr)
                return;
            for (auto& candidateTrack : candidate->tracks) {
                if (!candidateTrack->reuseRuntimeDevices)
                    continue;
                const auto existing = std::find_if(
                    timeline->tracks.begin(), timeline->tracks.end(),
                    [&candidateTrack](const auto& item) {
                        return item->id == candidateTrack->id
                            && item->effectTopologySignature
                                == candidateTrack->effectTopologySignature
                            && item->instrumentTopologySignature
                                == candidateTrack->instrumentTopologySignature;
                    });
                if (existing == timeline->tracks.end())
                    continue;
                (*existing)->effectChain = std::move(candidateTrack->effectChain);
                (*existing)->liveEffectChain = std::move(candidateTrack->liveEffectChain);
                (*existing)->recordingCapture.effectChain =
                    std::move(candidateTrack->recordingCapture.effectChain);
                (*existing)->instrumentRack = std::move(candidateTrack->instrumentRack);
            }
        };
        if (!applyPendingState()) {
            restoreRuntimeDevices();
            pendingTimeline = std::move(candidate);
            return false;
        }

        retiredTimeline = std::move(timeline);
        timeline = std::move(candidate);
        monitorLiveInput.store(pendingMonitorLiveInput, std::memory_order_release);
        armedInstrumentTrack.store(pendingArmedInstrumentTrack, std::memory_order_release);
        if (!hasExistingTimeline)
            timelineSample.store(0, std::memory_order_release);
        discontinuity.fetch_add(1, std::memory_order_relaxed);
        sequence.fetch_add(1, std::memory_order_relaxed);
    }
    return true;
}

void TimelineEngine::discardPreparedSnapshot() noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    pendingTimeline.reset();
}

void TimelineEngine::play() noexcept {
    state.store(State::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(State::stopped, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        resetPlaybackTrackState(*timeline);
        resetRecordingTrackState(*timeline);
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        resetPlaybackTrackState(*timeline);
        resetRecordingTrackState(*timeline);
    }
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    timelineSample.store(
        timeline->timebase.tickToSample(tick, timeline->outputSampleRate),
        std::memory_order_release);
    resetPlaybackTrackState(*timeline);
    resetRecordingTrackState(*timeline);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::startRecording(const int countInBeats, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr || timeline->outputSampleRate <= 0.0) {
        error = "Arrange recording requires a prepared Arrangement Graph.";
        return false;
    }
    if (recordingPhase.load(std::memory_order_acquire) != RecordingPhase::idle) {
        error = "Arrange recording is already active.";
        return false;
    }
    for (auto& track : timeline->tracks) {
        recordingCapture->resetTrack(track->recordingCapture);
        if (!track->instrument)
            track->recordingCapture.effectChain.reset();
    }
    recordingCapture->resetDrainingTailTracks();
    recordingCapture->resetCaptureErrors();
    recordingPassOrdinal.store(1, std::memory_order_release);
    const auto alreadyPlaying =
        state.load(std::memory_order_acquire) == State::playing;
    if (alreadyPlaying || countInBeats <= 0) {
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        recordingStartAudioSample.store(
            audioClockSample.load(std::memory_order_acquire), std::memory_order_release);
        const auto tick = timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire),
            timeline->outputSampleRate);
        recordingStartTick.store(tick, std::memory_order_release);
        if (!alreadyPlaying)
            state.store(State::playing, std::memory_order_release);
    } else {
        countInRemainingSamples.store(
            timeline->beatSamples * std::max(0, countInBeats), std::memory_order_release);
        recordingPhase.store(RecordingPhase::countingIn, std::memory_order_release);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::stopRecording() noexcept {
    recordingPhase.store(RecordingPhase::stopping, std::memory_order_release);
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    const auto hasCaptureWork = timeline != nullptr
        && std::any_of(timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
               return recordingCapture->hasCaptureWork(track->recordingCapture);
           });
    if (!hasCaptureWork)
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

bool TimelineEngine::cancelRecordingIfCountingIn() noexcept {
    auto expected = RecordingPhase::countingIn;
    if (!recordingPhase.compare_exchange_strong(
            expected,
            RecordingPhase::idle,
            std::memory_order_acq_rel))
        return false;
    countInRemainingSamples.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::flushRecordingTail(juce::String& error) noexcept {
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        auto sinkLease = recordingCapture->acquireSink();
        auto* sink = sinkLease.get();
        if (timeline == nullptr || sink == nullptr) {
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return true;
        }
        if (timeline->loopEnabled) {
            // Loop recording: close any active segment, then generate processed offline
            for (auto& trackPtr : timeline->tracks) {
                auto& track = *trackPtr;
                if (!track.armed || track.instrument
                    || track.recordingCapture.state != RecordingCaptureState::capturing)
                    continue;
                (void) recordingCapture->endTrackCapture(
                    track.id, track.recordingCapture);
                track.recordingCapture.state = RecordingCaptureState::idle;
            }
            if (!generateLoopProcessedVariants(*timeline, sink)) {
                error = "Loop recording Processed Variant generation failed.";
                return false;
            }
            recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
            return recordingCapture->captureErrors() == 0;
        }
        for (auto& trackPtr : timeline->tracks) {
            auto& track = *trackPtr;
            if (!track.armed || track.instrument
                || track.recordingCapture.state != RecordingCaptureState::capturing)
                continue;
            if (!recordingCapture->beginTailDrain(
                    track.id,
                    track.recordingCapture,
                    track.pluginDelaySamples,
                    track.pluginTailSamples)) {
                error = "Recording Capture Segment could not be closed for tail drain.";
                return false;
            }
        }
    }

    const auto deadline = juce::Time::getMillisecondCounter() + 5000u;
    while (recordingCapture->drainingTailTracks() != 0) {
        if (juce::Time::getMillisecondCounter() >= deadline) {
            error = "Processed recording tail did not drain before the realtime deadline.";
            return false;
        }
        juce::Thread::sleep(1);
    }
    recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
    return recordingCapture->captureErrors() == 0;
}

bool TimelineEngine::generateLoopProcessedVariants(
    PreparedTimeline& prepared,
    ArrangementCaptureSink* const sink) noexcept {
    const auto blockSize = std::max(1, prepared.preparedBlockSize);
    juce::AudioFormatManager formatReader;
    formatReader.registerBasicFormats();
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.instrument || !track.armed)
            continue;
        const auto rawFile = sink->prepareRawForReading(track.id);
        if (rawFile == juce::File {})
            continue;
        const auto segments = sink->getRawSegmentRanges(track.id);
        if (segments.empty())
            continue;
        // Open the flushed raw file as a stream so that non-.wav extensions
        // (e.g. .partial) are accepted by the AudioFormatManager readers.
        auto rawStream = rawFile.createInputStream();
        if (rawStream == nullptr || !rawStream->openedOk())
            return false;
        std::unique_ptr<juce::AudioFormatReader> reader(
            formatReader.createReaderFor(std::move(rawStream)));
        if (reader == nullptr)
            return false;
        const auto delay = static_cast<int>(std::max<std::int64_t>(
            0, track.pluginDelaySamples));
        for (const auto& [segStart, segEnd] : segments) {
            const auto segmentSamples = static_cast<int>(segEnd - segStart);
            if (segmentSamples <= 0)
                continue;
            track.recordingCapture.effectChain.reset();
            // Output buffer holds segmentSamples + delay for latency alignment
            const auto outputSize = segmentSamples + delay;
            juce::AudioBuffer<float> outputBuffer(2, outputSize);
            outputBuffer.clear();
            int outputOffset = 0;
            // Process raw segment in block-sized chunks
            juce::AudioBuffer<float> blockBuffer(2, blockSize);
            int remaining = segmentSamples;
            std::int64_t readPos = static_cast<std::int64_t>(segStart);
            while (remaining > 0) {
                const auto count = std::min(blockSize, remaining);
                blockBuffer.clear();
                reader->read(
                    blockBuffer.getArrayOfWritePointers(), 2,
                    readPos, count);
                readPos += count;
                const std::array<float*, 2> outPtrs {
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(
                    blockBuffer.getArrayOfReadPointers(),
                    2,
                    outPtrs.data(),
                    2,
                    count);
                outputOffset += count;
                remaining -= count;
            }
            // Feed delay zeros to flush plugin latency
            int delayRemaining = delay;
            while (delayRemaining > 0) {
                const auto count = std::min(blockSize, delayRemaining);
                blockBuffer.clear();
                const std::array<float*, 2> outPtrs {
                    outputBuffer.getWritePointer(0) + outputOffset,
                    outputBuffer.getWritePointer(1) + outputOffset,
                };
                track.recordingCapture.effectChain.process(
                    blockBuffer.getArrayOfReadPointers(),
                    2,
                    outPtrs.data(),
                    2,
                    count);
                outputOffset += count;
                delayRemaining -= count;
            }
            // Discard first delay samples, write segmentSamples in block-sized chunks
            constexpr int kOfflineWriterTimeoutMs = 5000;
            int writeOffset = 0;
            while (writeOffset < segmentSamples) {
                const auto count = std::min(
                    blockSize,
                    segmentSamples - writeOffset);
                const std::array<const float*, 2> processedBlock {
                    outputBuffer.getReadPointer(0) + delay + writeOffset,
                    outputBuffer.getReadPointer(1) + delay + writeOffset,
                };
                if (!sink->writeProcessedAudioTrackOffline(
                        track.id,
                        processedBlock.data(),
                        count,
                        kOfflineWriterTimeoutMs)) {
                    return false;
                }
                writeOffset += count;
            }
        }
    }
    return true;
}

juce::var TimelineEngine::recordingConfiguration() const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr)
        return {};
    auto* result = new juce::DynamicObject();
    result->setProperty("sampleRate", timeline->outputSampleRate);
    const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
        timelineSample.load(std::memory_order_acquire),
        timeline->outputSampleRate));
    result->setProperty("timelineStartTick", tick);
    result->setProperty("loopEnabled", timeline->loopEnabled);
    result->setProperty("loopStartSample", static_cast<juce::int64>(timeline->loopStartSample));
    result->setProperty("loopEndSample", static_cast<juce::int64>(timeline->loopEndSample));
    result->setProperty("punchEnabled", timeline->punchEnabled);
    result->setProperty("punchStartSample", static_cast<juce::int64>(timeline->punchStartSample));
    result->setProperty("punchEndSample", static_cast<juce::int64>(timeline->punchEndSample));
    juce::Array<juce::var> trackValues;
    for (const auto& track : timeline->tracks) {
        if (!track->armed)
            continue;
        auto* value = new juce::DynamicObject();
        value->setProperty("trackId", track->id);
        value->setProperty("kind", track->instrument ? "instrument" : "audio");
        value->setProperty("audioInputChannel", track->audioInputChannel);
        value->setProperty("midiDeviceId", track->midiDeviceId);
        value->setProperty("midiChannel", track->midiChannel);
        value->setProperty(
            "pluginLatencySamples", static_cast<int>(track->pluginDelaySamples));
        value->setProperty(
            "pluginTailSamples", static_cast<int>(track->pluginTailSamples));
        trackValues.add(juce::var(value));
    }
    result->setProperty("tracks", trackValues);
    return juce::var(result);
}

void TimelineEngine::setRecordingSink(ArrangementCaptureSink* const sink) noexcept {
    recordingCapture->setSink(sink);
}

void TimelineEngine::clearRecordingSink() noexcept {
    recordingCapture->clearSink();
}

bool TimelineEngine::enqueueLiveMidi(
    const juce::MidiMessage& message,
    const juce::String& deviceId) noexcept {
    if (!armedInstrumentTrack.load(std::memory_order_acquire))
        return false;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr)
        return true;
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (track.instrument && track.armed
            && ArrangementGraph::midiRouteMatches(
                track.midiDeviceId, track.midiChannel, deviceId, message.getChannel())) {
            if (track.instrumentRack != nullptr)
                track.instrumentRack->enqueueMidi(message);
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(
                    track.id,
                    deviceId,
                    message,
                    audioClockSample.load(std::memory_order_acquire));
            }
        }
    }
    return true;
}

PluginRack* TimelineEngine::findDevice(
    const juce::String& trackId,
    const juce::String& deviceId) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr)
        return nullptr;
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end())
        return nullptr;
    auto& track = **found;
    const auto instrument = track.instrumentRack.get();
    if (instrument != nullptr && deviceId == track.instrumentDeviceId)
        return instrument;
    return track.effectChain.findDevice(deviceId);
}

bool TimelineEngine::mirrorEditorDeviceState(
    const juce::String& trackId,
    const juce::String& deviceId,
    const juce::var& persistedState,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return true;
    auto* live = track.liveEffectChain.findDevice(deviceId);
    if (live == nullptr) {
        error = "Live Track Device was not found.";
        return false;
    }
    if (!live->applyPersistedState(persistedState, error))
        return false;
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (recording != nullptr && !recording->applyPersistedState(persistedState, error))
        return false;
    return true;
}

bool TimelineEngine::mirrorEditorDeviceParameter(
    const juce::String& trackId,
    const juce::String& deviceId,
    const int parameterIndex,
    const float value,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return true;
    auto* live = track.liveEffectChain.findDevice(deviceId);
    if (live == nullptr) {
        error = "Live Track Device was not found.";
        return false;
    }
    live->enqueueParameterChange(parameterIndex, value);
    if (auto* recording = track.recordingCapture.effectChain.findDevice(deviceId))
        recording->enqueueParameterChange(parameterIndex, value);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

juce::var TimelineEngine::devicePersistedState(
    const juce::String& trackId,
    const juce::String& deviceId,
    juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return {};
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(),
        [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId)
        return track.instrumentRack->persistedState(error);
    return track.effectChain.persistedState(deviceId, error);
}

bool TimelineEngine::preparedTrackReusesRuntimeDevices(
    const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (pendingTimeline == nullptr)
        return false;
    const auto track = std::find_if(
        pendingTimeline->tracks.begin(),
        pendingTimeline->tracks.end(),
        [&trackId](const auto& item) { return item->id == trackId; });
    return track != pendingTimeline->tracks.end()
        && (*track)->reuseRuntimeDevices;
}

bool TimelineEngine::setDeviceBypassed(
    const juce::String& trackId,
    const juce::String& deviceId,
    const bool bypassed,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId) {
        track.instrumentRack->setBypassed(bypassed);
    } else {
        auto* playback = track.effectChain.findDevice(deviceId);
        auto* live = track.liveEffectChain.findDevice(deviceId);
        auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
        if (playback == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        playback->setBypassed(bypassed);
        if (live != nullptr)
            live->setBypassed(bypassed);
        if (recording != nullptr)
            recording->setBypassed(bypassed);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceParameter(
    const juce::String& trackId,
    const juce::String& deviceId,
    const int parameterIndex,
    const float value,
    juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(
        timeline->tracks.begin(), timeline->tracks.end(), [&](const auto& track) {
            return track->id == trackId;
        });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    auto* playback = track.instrumentRack != nullptr && track.instrumentDeviceId == deviceId
        ? track.instrumentRack.get()
        : track.effectChain.findDevice(deviceId);
    auto* live = track.liveEffectChain.findDevice(deviceId);
    auto* recording = track.recordingCapture.effectChain.findDevice(deviceId);
    if (playback == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    const auto parameterStatus = playback->parameterStatus().getProperty("parameters", {});
    if (!parameterStatus.isArray() || parameterIndex < 0 || parameterIndex >= parameterStatus.size()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    const auto previous =
        static_cast<float>(parameterStatus[parameterIndex].getProperty("value", 0.0));
    if (!playback->setParameter(parameterIndex, value, error))
        return false;
    if (live != nullptr && !live->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void) playback->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    if (recording != nullptr && !recording->setParameter(parameterIndex, value, error)) {
        juce::String rollbackError;
        (void) playback->setParameter(parameterIndex, previous, rollbackError);
        if (live != nullptr)
            (void) live->setParameter(parameterIndex, previous, rollbackError);
        return false;
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return monitorLiveInput.load(std::memory_order_acquire);
}

bool TimelineEngine::recordingWindow(
    const int sampleCount,
    int& sampleOffset,
    int& capturedSamples) noexcept {
    sampleOffset = 0;
    capturedSamples = std::max(0, sampleCount);
    captureBlockOffset.store(0, std::memory_order_release);
    captureBlockSamples.store(0, std::memory_order_release);
    playbackBlockOffset.store(0, std::memory_order_release);
    countInBlockStartRemainingSamples.store(0, std::memory_order_release);
    if (sampleCount <= 0)
        return false;
    auto phase = recordingPhase.load(std::memory_order_acquire);
    auto transitionedFromCountIn = false;
    if (phase == RecordingPhase::idle || phase == RecordingPhase::stopping) {
        capturedSamples = 0;
        return false;
    }
    if (phase == RecordingPhase::countingIn) {
        const auto remaining = countInRemainingSamples.load(std::memory_order_acquire);
        countInBlockStartRemainingSamples.store(remaining, std::memory_order_release);
        if (remaining >= sampleCount) {
            countInRemainingSamples.store(
                remaining - sampleCount, std::memory_order_release);
            capturedSamples = 0;
            return false;
        }
        sampleOffset = static_cast<int>(std::max<std::int64_t>(0, remaining));
        playbackBlockOffset.store(sampleOffset, std::memory_order_release);
        capturedSamples = sampleCount - sampleOffset;
        countInRemainingSamples.store(0, std::memory_order_release);
        recordingStartAudioSample.store(
            audioClockSample.load(std::memory_order_acquire) +
                static_cast<std::uint64_t>(sampleOffset),
            std::memory_order_release);
        const juce::SpinLock::ScopedTryLockType lock(timelineLock);
        if (lock.isLocked() && timeline != nullptr) {
            const auto tick = timeline->timebase.sampleToTick(
                timelineSample.load(std::memory_order_acquire),
                timeline->outputSampleRate);
            recordingStartTick.store(tick, std::memory_order_release);
        }
        state.store(State::playing, std::memory_order_release);
        recordingPhase.store(RecordingPhase::recording, std::memory_order_release);
        phase = RecordingPhase::recording;
        transitionedFromCountIn = true;
    }

    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->punchEnabled) {
        captureBlockOffset.store(sampleOffset, std::memory_order_release);
        captureBlockSamples.store(capturedSamples, std::memory_order_release);
        return true;
    }

    const auto position = timelineSample.load(std::memory_order_acquire);
    const auto playbackOffset = transitionedFromCountIn ? sampleOffset : 0;
    const auto playbackSamples = sampleCount - playbackOffset;
    const auto blockEnd = position + static_cast<std::int64_t>(playbackSamples);
    if (blockEnd <= timeline->punchStartSample || position >= timeline->punchEndSample) {
        capturedSamples = 0;
        return false;
    }
    const auto punchOffset = static_cast<int>(std::max<std::int64_t>(
        0, timeline->punchStartSample - position));
    sampleOffset = playbackOffset + punchOffset;
    const auto end = std::min<std::int64_t>(blockEnd, timeline->punchEndSample);
    capturedSamples = static_cast<int>(std::max<std::int64_t>(
        0, end - position - punchOffset));
    captureBlockOffset.store(sampleOffset, std::memory_order_release);
    captureBlockSamples.store(capturedSamples, std::memory_order_release);
    return capturedSamples > 0;
}

void TimelineEngine::mixMetronome(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    if (sampleCount <= 0)
        return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr || !timeline->metronomeEnabled
        || timeline->beatSamples <= 0)
        return;
    const auto loopLength = timeline->loopEndSample - timeline->loopStartSample;
    const auto start = lastMixStartSample.load(std::memory_order_acquire);
    const auto playbackOffset = juce::jlimit(
        0, sampleCount, lastMixPlaybackOffset.load(std::memory_order_acquire));
    const auto countInRemaining =
        countInBlockStartRemainingSamples.load(std::memory_order_acquire);
    const auto countingIn =
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::countingIn;
    const auto playing = state.load(std::memory_order_acquire) == State::playing;
    constexpr std::int64_t clickSamples = 1'920;
    for (int sample = 0; sample < sampleCount; ++sample) {
        float value = 0.0f;
        if (countInRemaining > 0
            && sample < (countingIn ? sampleCount : playbackOffset)) {
            const auto remaining = countInRemaining - sample;
            const auto offset = (timeline->beatSamples
                - remaining % timeline->beatSamples) % timeline->beatSamples;
            if (offset >= 0 && offset < clickSamples) {
                const auto envelope = 1.0f - static_cast<float>(offset) / clickSamples;
                value = 0.11f * envelope;
            }
        } else if (playing && sample >= playbackOffset) {
            auto position = start + sample - playbackOffset;
            if (timeline->loopEnabled && loopLength > 0
                && position >= timeline->loopEndSample)
                position = timeline->loopStartSample +
                    (position - timeline->loopEndSample) % loopLength;
            if (position >= 0) {
                const auto beat = position / timeline->beatSamples;
                const auto offset = position % timeline->beatSamples;
                if (offset >= 0 && offset < clickSamples) {
                    const auto envelope =
                        1.0f - static_cast<float>(offset) / clickSamples;
                    const auto amplitude =
                        beat % timeline->beatsPerBar == 0 ? 0.18f : 0.11f;
                    value = amplitude * envelope;
                }
            }
        }
        if (value <= 0.0f)
            continue;
        for (int channel = 0; channel < channelCount; ++channel) {
            if (outputChannels[channel] != nullptr)
                outputChannels[channel][sample] += value;
        }
    }
}

void TimelineEngine::mixRange(
    Track& track,
    const std::int64_t rangeStart,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : track.clips) {
        auto& clip = *clipPtr;
        if (clip.muted) continue;
        const auto clipEnd = clip.startSample + clip.durationSamples;
        const auto overlapStart = std::max(rangeStart, clip.startSample);
        const auto overlapEnd = std::min(rangeEnd, clipEnd);
        if (overlapEnd <= overlapStart) continue;
        auto remaining = static_cast<int>(overlapEnd - overlapStart);
        auto outputOffset = destinationStart + static_cast<int>(overlapStart - rangeStart);
        auto localSample = overlapStart - clip.startSample;
        while (remaining > 0) {
            const auto sourceRange = clip.sourceEndFrame - clip.sourceStartFrame;
            auto sourceOffset = static_cast<std::int64_t>(std::floor(
                static_cast<double>(localSample) * clip.sourceSampleRate /
                track.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd = static_cast<int>(std::ceil(
                static_cast<double>(sourceRemaining) * track.outputSampleRate /
                clip.sourceSampleRate));
            const auto chunk = std::min(remaining, std::max(1, outputUntilSourceEnd));
            if (clip.expectedSourceFrame < 0 ||
                std::abs(clip.expectedSourceFrame - sourceFrame) > 2) {
                clip.transport.setPosition(
                    static_cast<double>(sourceFrame) / clip.sourceSampleRate);
            }
            clip.scratch.clear();
            clip.transport.getNextAudioBlock(
                juce::AudioSourceChannelInfo(&clip.scratch, 0, chunk));
            for (int sample = 0; sample < chunk; ++sample) {
                const auto position = localSample + sample;
                auto envelope = 1.0f;
                if (clip.fadeInSamples > 0 && position < clip.fadeInSamples) {
                    const auto progress = static_cast<float>(position) /
                        static_cast<float>(clip.fadeInSamples);
                    envelope = std::min(
                        envelope,
                        std::sin(juce::MathConstants<float>::halfPi * progress));
                }
                const auto remainingClip = clip.durationSamples - position - 1;
                if (clip.fadeOutSamples > 0 && remainingClip < clip.fadeOutSamples) {
                    const auto progress = static_cast<float>(
                        std::max<std::int64_t>(0, remainingClip)) /
                        static_cast<float>(clip.fadeOutSamples);
                    envelope = std::min(
                        envelope,
                        std::sin(juce::MathConstants<float>::halfPi * progress));
                }
                const auto panAngle = (clip.pan + 1.0f) *
                    juce::MathConstants<float>::pi * 0.25f;
                const auto source = clip.scratch.getSample(0, sample) * clip.gain * envelope;
                track.mixBuffer.addSample(
                    0, outputOffset + sample, source * std::cos(panAngle));
                track.mixBuffer.addSample(
                    1, outputOffset + sample,
                    clip.scratch.getNumChannels() > 1
                        ? clip.scratch.getSample(1, sample) * clip.gain * envelope * std::sin(panAngle)
                        : source * std::sin(panAngle));
            }
            clip.expectedSourceFrame = sourceFrame + static_cast<std::int64_t>(std::floor(
                static_cast<double>(chunk) * clip.sourceSampleRate /
                track.outputSampleRate));
            remaining -= chunk;
            outputOffset += chunk;
            localSample += chunk;
            if (!clip.loop && sourceFrame + sourceRemaining >= clip.sourceEndFrame && remaining > 0)
                break;
            if (clip.loop && remaining > 0) clip.expectedSourceFrame = -1;
        }
    }
}

void TimelineEngine::scheduleMidi(
    const PreparedTimeline& prepared,
    Track& track,
    const std::int64_t rangeStart,
    const int sampleCount) noexcept {
    track.midiBuffer.clear();
    const auto rangeEnd = rangeStart + sampleCount;
    for (const auto& clip : track.midiClips) {
        if (clip.muted) continue;
        const auto clipStart = prepared.timebase.tickToSample(
            clip.startTick, track.outputSampleRate);
        const auto clipLength = std::max<std::int64_t>(1, prepared.timebase.tickToSample(
            clip.durationTicks, track.outputSampleRate));
        const auto firstIteration = clip.loop && rangeStart > clipStart
            ? std::max<std::int64_t>(0, (rangeStart - clipStart) / clipLength - 1)
            : 0;
        const auto lastIteration = clip.loop
            ? std::max<std::int64_t>(firstIteration,
                (rangeEnd - clipStart) / clipLength + 1)
            : 0;
        const auto addMessage = [&](const juce::MidiMessage& message, const std::int64_t sample) {
            if (sample >= rangeStart && sample < rangeEnd)
                track.midiBuffer.addEvent(
                    message, juce::jlimit(0, sampleCount - 1,
                        static_cast<int>(sample - rangeStart)));
        };
        for (std::int64_t iteration = firstIteration; iteration <= lastIteration; ++iteration) {
            const auto iterationStart = clipStart + iteration * clipLength;
            for (const auto& note : clip.notes) {
                const auto noteStart = iterationStart + prepared.timebase.tickToSample(
                    note.startTick, track.outputSampleRate);
                const auto noteEnd = std::min(
                    iterationStart + clipLength,
                    noteStart + std::max<std::int64_t>(1, prepared.timebase.tickToSample(
                        note.durationTicks, track.outputSampleRate)));
                addMessage(juce::MidiMessage::noteOn(
                    juce::jlimit(1, 16, note.channel), note.note,
                    static_cast<juce::uint8>(juce::jlimit(1, 127, note.velocity))), noteStart);
                addMessage(juce::MidiMessage::noteOff(
                    juce::jlimit(1, 16, note.channel), note.note), noteEnd);
            }
            for (const auto& event : clip.events) {
                const auto eventSample = iterationStart + prepared.timebase.tickToSample(
                    event.tick, track.outputSampleRate);
                const auto channel = juce::jlimit(1, 16, event.channel);
                if (event.kind == "controlChange")
                    addMessage(juce::MidiMessage::controllerEvent(
                        channel, event.data1, event.data2), eventSample);
                else if (event.kind == "pitchBend")
                    addMessage(juce::MidiMessage::pitchWheel(
                        channel, event.data1 | (event.data2 << 7)), eventSample);
                else if (event.kind == "channelPressure")
                    addMessage(juce::MidiMessage::channelPressureChange(
                        channel, event.data1), eventSample);
            }
            if (!clip.loop) break;
        }
    }
}

void TimelineEngine::processTracks(
    PreparedTimeline& prepared,
    const float* const* physicalInputChannels,
    const int physicalInputChannelCount,
    float* const* outputChannels,
    const int channelCount,
    const std::int64_t rangeStart,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(
        prepared.tracks.begin(), prepared.tracks.end(),
        [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        const auto audible = !track.muted && (!hasSolo || track.solo);
        track.processedBuffer.clear(0, sampleCount);
        const float* inputChannels[2] = {
            track.mixBuffer.getWritePointer(0), track.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {
            track.processedBuffer.getWritePointer(0), track.processedBuffer.getWritePointer(1)};
        if (track.instrument) {
            if (track.instrumentRack != nullptr)
                track.instrumentRack->process(
                    nullptr, 0, track.mixBuffer.getArrayOfWritePointers(), 2, sampleCount,
                    &track.midiBuffer);
            else
                track.mixBuffer.clear(0, sampleCount);
            track.effectChain.process(
                track.mixBuffer.getArrayOfReadPointers(),
                2,
                processedChannels,
                2,
                sampleCount);
        } else {
            track.effectChain.process(
                inputChannels, 2, processedChannels, 2, sampleCount);
        }
        const auto delay = track.compensationDelaySamples;
        const auto delaySize = track.delayBuffer.getNumSamples();
        for (int sample = 0; sample < sampleCount; ++sample) {
            const auto timelinePosition = rangeStart + sample;
            const auto gain = juce::Decibels::decibelsToGain(
                ArrangementGraph::automationValueAt(
                    track.volumeAutomation, timelinePosition, track.gainDb));
            const auto pan = juce::jlimit(
                -1.0f,
                1.0f,
                ArrangementGraph::automationValueAt(
                    track.panAutomation, timelinePosition, track.pan));
            const auto panAngle =
                (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
            const auto leftGain = gain * std::cos(panAngle);
            const auto rightGain = gain * std::sin(panAngle);
            float left = processedChannels[0][sample];
            float right = processedChannels[1][sample];
            if (delay > 0 && delaySize > 0) {
                const auto write = track.delayWritePosition;
                track.delayBuffer.setSample(0, static_cast<int>(write), left);
                track.delayBuffer.setSample(1, static_cast<int>(write), right);
                const auto read = (write - delay + delaySize) % delaySize;
                left = track.delayBuffer.getSample(0, static_cast<int>(read));
                right = track.delayBuffer.getSample(1, static_cast<int>(read));
                track.delayWritePosition = (write + 1) % delaySize;
            }
            if (audible && channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + sample] += left * leftGain;
            if (audible && channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + sample] += right * rightGain;
        }
        if (!track.instrument && (track.monitorInput || track.armed)
            && track.audioInputChannel >= 0) {
            const auto* source = ArrangementGraph::audioInputSource(
                track.audioInputChannel,
                physicalInputChannels,
                physicalInputChannelCount);
            for (int channel = 0; channel < 2; ++channel) {
                auto* destination = track.liveInputBuffer.getWritePointer(channel);
                if (source != nullptr)
                    juce::FloatVectorOperations::copy(
                        destination, source + destinationStart, sampleCount);
                else
                    juce::FloatVectorOperations::clear(destination, sampleCount);
            }
            track.liveEffectChain.process(
                track.liveInputBuffer.getArrayOfReadPointers(),
                2,
                track.liveProcessedBuffer.getArrayOfWritePointers(),
                2,
                sampleCount);
            const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
            const auto captureEnd =
                captureStart + captureBlockSamples.load(std::memory_order_acquire);
            const auto [writeStart, writeEnd] = ArrangementGraph::captureIntersection(
                destinationStart, sampleCount, captureStart, captureEnd - captureStart);
            if (track.armed) {
                auto& capture = track.recordingCapture;
                if (writeEnd > writeStart) {
                    const auto localOffset = writeStart - destinationStart;
                    const auto captureAudioStart = callbackAudioStartSample.load(
                        std::memory_order_acquire) + static_cast<std::uint64_t>(writeStart);
                    const auto captureTimelineStart = static_cast<std::uint64_t>(
                        rangeStart + localOffset);
                    const auto discontinuous =
                        capture.state != RecordingCaptureState::capturing
                        || captureAudioStart != capture.endAudioSample;
                    if (discontinuous
                        && capture.state == RecordingCaptureState::capturing) {
                        if (!recordingCapture->beginTailDrain(
                                track.id,
                                capture,
                                track.pluginDelaySamples,
                                track.pluginTailSamples))
                            capture.state = RecordingCaptureState::completed;
                    }
                    if (capture.state == RecordingCaptureState::idle) {
                        capture.effectChain.reset();
                        capture.latencyToDiscard = static_cast<int>(std::max<std::int64_t>(
                            0, track.pluginDelaySamples));
                        (void) recordingCapture->beginTrackCapture(
                            track.id,
                            capture,
                            captureAudioStart,
                            captureTimelineStart);
                    }
                    if (capture.state == RecordingCaptureState::capturing) {
                        const auto writeCount = writeEnd - writeStart;
                        const auto* rawPointer =
                            track.liveInputBuffer.getReadPointer(0) + localOffset;
                        if (prepared.loopEnabled) {
                            // Loop recording: write raw only (processed generated offline)
                            recordingCapture->writeAudioTrack(
                                track.id, rawPointer, writeCount, nullptr, 0);
                        } else {
                            // Normal recording: write raw + processed in real-time
                            capture.processedBuffer.clear(0, writeCount);
                            const std::array<const float*, 2> recordingInput {
                                track.liveInputBuffer.getReadPointer(0) + localOffset,
                                track.liveInputBuffer.getReadPointer(1) + localOffset,
                            };
                            capture.effectChain.process(
                                recordingInput.data(),
                                2,
                                capture.processedBuffer.getArrayOfWritePointers(),
                                2,
                                writeCount);
                            const auto discard = std::min(
                                capture.latencyToDiscard, writeCount);
                            capture.latencyToDiscard -= discard;
                            const auto processedCount = writeCount - discard;
                            const std::array<const float*, 2> processed {
                                capture.processedBuffer.getReadPointer(0) + discard,
                                capture.processedBuffer.getReadPointer(1) + discard,
                            };
                            recordingCapture->writeAudioTrack(
                                track.id, rawPointer, writeCount,
                                processed.data(), processedCount);
                        }
                        capture.endAudioSample = captureAudioStart
                            + static_cast<std::uint64_t>(writeCount);
                        capture.endTimelineSample = captureTimelineStart
                            + static_cast<std::uint64_t>(writeCount);
                    }
                } else if (capture.state == RecordingCaptureState::capturing) {
                    if (!recordingCapture->beginTailDrain(
                            track.id,
                            capture,
                            track.pluginDelaySamples,
                            track.pluginTailSamples))
                        capture.state = RecordingCaptureState::completed;
                }
            }
            if (track.monitorInput && audible) {
                for (int sample = 0; sample < sampleCount; ++sample) {
                    const auto timelinePosition = rangeStart + sample;
                    const auto gain = juce::Decibels::decibelsToGain(
                        ArrangementGraph::automationValueAt(
                            track.volumeAutomation, timelinePosition, track.gainDb));
                    const auto pan = juce::jlimit(
                        -1.0f,
                        1.0f,
                        ArrangementGraph::automationValueAt(
                            track.panAutomation, timelinePosition, track.pan));
                    const auto panAngle =
                        (pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
                    if (channelCount > 0 && outputChannels[0] != nullptr)
                        outputChannels[0][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(0, sample)
                            * gain * std::cos(panAngle);
                    if (channelCount > 1 && outputChannels[1] != nullptr)
                        outputChannels[1][destinationStart + sample] +=
                            track.liveProcessedBuffer.getSample(1, sample)
                            * gain * std::sin(panAngle);
                }
            }
        }
    }
}

void TimelineEngine::resetPlaybackTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        track.mixBuffer.clear();
        track.processedBuffer.clear();
        track.midiBuffer.clear();
        if (track.instrumentRack != nullptr)
            track.instrumentRack->allNotesOff();
        track.effectChain.allNotesOff();
        track.liveEffectChain.allNotesOff();
        track.delayBuffer.clear();
        track.delayWritePosition = 0;
    }
}

void TimelineEngine::resetRecordingTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        recordingCapture->resetTrack(track.recordingCapture);
    }
    recordingCapture->resetDrainingTailTracks();
}

void TimelineEngine::mix(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    mix(nullptr, 0, outputChannels, channelCount, sampleCount);
}

void TimelineEngine::mix(
    const float* const* inputChannels,
    const int inputChannelCount,
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    audioClockSample.fetch_add(static_cast<std::uint64_t>(sampleCount), std::memory_order_relaxed);
    callbackAudioStartSample.store(
        audioClockSample.load(std::memory_order_acquire) - static_cast<std::uint64_t>(sampleCount),
        std::memory_order_release);
    const auto blockPlaybackOffset = juce::jlimit(
        0, sampleCount, playbackBlockOffset.exchange(0, std::memory_order_acq_rel));
    lastMixPlaybackOffset.store(blockPlaybackOffset, std::memory_order_release);
    if (state.load(std::memory_order_acquire) != State::playing) return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked()) {
        callbackLockMisses.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (timeline == nullptr) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    lastMixStartSample.store(position, std::memory_order_release);
    if (recordingCapture->drainingTailTracks() != 0) {
        for (auto& trackPtr : timeline->tracks)
            (void) recordingCapture->drainTail(
                trackPtr->id,
                trackPtr->recordingCapture,
                trackPtr->liveInputBuffer,
                sampleCount);
    }
    if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::stopping) {
        if (recordingCapture->drainingTailTracks() != 0)
            return;
        recordingPhase.store(RecordingPhase::idle, std::memory_order_release);
        return;
    }
    auto consumed = blockPlaybackOffset;
    while (consumed < sampleCount) {
        auto chunk = sampleCount - consumed;
        if (!timeline->tracks.empty()) {
            const auto bufferSize = timeline->tracks.front()->mixBuffer.getNumSamples();
            if (bufferSize > 0)
                chunk = std::min(chunk, bufferSize);
        }
        if (timeline->loopEnabled && position < timeline->loopEndSample)
            chunk = std::min<int>(chunk, static_cast<int>(timeline->loopEndSample - position));
        for (auto& trackPtr : timeline->tracks)
            trackPtr->mixBuffer.clear(0, chunk);
        for (auto& trackPtr : timeline->tracks)
            mixRange(*trackPtr, position, 0, chunk);
        for (auto& trackPtr : timeline->tracks)
            scheduleMidi(*timeline, *trackPtr, position, chunk);
        const auto captureStart = captureBlockOffset.load(std::memory_order_acquire);
        const auto captureSamples = captureBlockSamples.load(std::memory_order_acquire);
        const auto [captureWriteStart, captureWriteEnd] =
            ArrangementGraph::captureIntersection(
                consumed, chunk, captureStart, captureSamples);
        if (captureWriteEnd > captureWriteStart
            && recordingPhase.load(std::memory_order_acquire)
                == RecordingPhase::recording) {
            const auto callbackStart = audioClockSample.load(std::memory_order_acquire)
                - static_cast<std::uint64_t>(sampleCount);
            const auto localOffset = captureWriteStart - consumed;
            recordingCapture->setCaptureRange(
                callbackStart + static_cast<std::uint64_t>(captureWriteStart),
                callbackStart + static_cast<std::uint64_t>(captureWriteEnd),
                static_cast<std::uint64_t>(position)
                    + static_cast<std::uint64_t>(localOffset),
                static_cast<std::uint64_t>(position)
                    + static_cast<std::uint64_t>(
                        localOffset + captureWriteEnd - captureWriteStart));
        }
        processTracks(
            *timeline,
            inputChannels,
            inputChannelCount,
            outputChannels,
            channelCount,
            position,
            consumed,
            chunk);
        position += chunk;
        consumed += chunk;
        // Decrement the capture budget so recording stops at the window end
        {
            auto remaining = captureBlockSamples.load(std::memory_order_acquire);
            if (remaining > 0)
                captureBlockSamples.store(
                    remaining - std::min(chunk, remaining), std::memory_order_release);
        }
        if (timeline->loopEnabled && position >= timeline->loopEndSample) {
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                const auto callbackStart = audioClockSample.load(std::memory_order_acquire)
                    - static_cast<std::uint64_t>(sampleCount);
                recordingCapture->markLoopBoundary(
                    callbackStart + static_cast<std::uint64_t>(consumed));
                for (auto& trackPtr : timeline->tracks) {
                    auto& track = *trackPtr;
                    if (!track.armed || track.instrument
                        || track.recordingCapture.state != RecordingCaptureState::capturing)
                        continue;
                    (void) recordingCapture->endTrackCapture(
                        track.id, track.recordingCapture);
                    track.recordingCapture.state = RecordingCaptureState::idle;
                    track.recordingCapture.effectChain.reset();
                    track.recordingCapture.latencyToDiscard = 0;
                }
            }
            position = timeline->loopStartSample;
            recordingPassOrdinal.fetch_add(1, std::memory_order_relaxed);
            resetPlaybackTrackState(*timeline);
            discontinuity.fetch_add(1, std::memory_order_relaxed);
        }
    }
    timelineSample.store(position, std::memory_order_release);
}

juce::var TimelineEngine::status() const {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "transportStatus");
    const auto currentState = state.load(std::memory_order_acquire);
    object->setProperty(
        "state",
        currentState == State::playing ? "playing" :
        currentState == State::faulted ? "faulted" : "stopped");
    object->setProperty("timelineSample", static_cast<juce::int64>(
        timelineSample.load(std::memory_order_acquire)));
    object->setProperty("audioClockSample", static_cast<juce::int64>(
        audioClockSample.load(std::memory_order_acquire)));
    object->setProperty("sequence", static_cast<juce::int64>(
        sequence.fetch_add(1, std::memory_order_relaxed) + 1));
    object->setProperty("callbackLockMisses", static_cast<juce::int64>(
        callbackLockMisses.load(std::memory_order_acquire)));
    object->setProperty("clockGeneration", static_cast<juce::int64>(
        clockGeneration.load(std::memory_order_acquire)));
    object->setProperty("discontinuity", static_cast<juce::int64>(
        discontinuity.load(std::memory_order_acquire)));
    object->setProperty("revision", 0);
    object->setProperty("sampleRate", 0.0);
    object->setProperty("timelineTick", 0);
    const auto phase = recordingPhase.load(std::memory_order_acquire);
    object->setProperty(
        "recordingPhase",
        phase == RecordingPhase::countingIn ? "countingIn" :
        phase == RecordingPhase::recording ? "recording" :
        phase == RecordingPhase::stopping ? "stopping" : "idle");
    object->setProperty("recordingStartTick", static_cast<juce::int64>(
        recordingStartTick.load(std::memory_order_acquire)));
    object->setProperty("recordingPassOrdinal", static_cast<int>(
        recordingPassOrdinal.load(std::memory_order_acquire)));
    object->setProperty("recordingCaptureErrors", static_cast<juce::int64>(
        recordingCapture->captureErrors()));
    object->setProperty("drainingTailTracks", static_cast<int>(
        recordingCapture->drainingTailTracks()));
    object->setProperty("unavailableClipIds", juce::Array<juce::var> {});
    object->setProperty("missingDeviceIds", juce::Array<juce::var> {});
    juce::Array<juce::var> armedTrackIds;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        const auto tick = static_cast<juce::int64>(timeline->timebase.sampleToTick(
            timelineSample.load(std::memory_order_acquire),
            timeline->outputSampleRate));
        object->setProperty("timelineTick", tick);
        object->setProperty("recordingCurrentTick", tick);
        object->setProperty("unavailableClipIds", timeline->unavailableClipIds);
        object->setProperty("missingDeviceIds", timeline->missingDeviceIds);
        for (const auto& track : timeline->tracks)
            if (track->armed)
                armedTrackIds.add(track->id);
    }
    object->setProperty("armedTrackIds", armedTrackIds);
    return juce::var(object);
}

juce::var runTimelineSelfTest(const juce::File& directory) {
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "timelineSelfTest");
    juce::Array<juce::var> checks;
    const auto mono = directory.getChildFile("timeline-44100-mono.wav");
    const auto stereo = directory.getChildFile("timeline-48000-stereo.wav");
    directory.createDirectory();
    const auto sourcesWritten =
        writePcmWave(mono, 44100, 1, 44100, 6000) &&
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
        auto* first = new juce::DynamicObject();
        first->setProperty("id", "device:test");
        first->setProperty("kind", "plugin");
        first->setProperty("path", "C:\\test\\Effect.vst3");
        first->setProperty("bypassed", false);
        juce::Array<juce::var> firstParameters;
        firstParameters.add(0.1);
        firstParameters.add(0.2);
        first->setProperty("parameterValues", firstParameters);
        auto* second = new juce::DynamicObject();
        second->setProperty("id", "device:test");
        second->setProperty("kind", "plugin");
        second->setProperty("path", "C:\\test\\Effect.vst3");
        second->setProperty("bypassed", true);
        juce::Array<juce::var> secondParameters;
        secondParameters.add(0.8);
        secondParameters.add(0.9);
        second->setProperty("parameterValues", secondParameters);
        juce::Array<juce::var> firstChain;
        firstChain.add(juce::var(first));
        juce::Array<juce::var> secondChain;
        secondChain.add(juce::var(second));
        mutablePluginStateKeepsTopology =
            pluginTopologySignature(juce::var(firstChain))
            == pluginTopologySignature(juce::var(secondChain));
    }
    if (sourcesWritten) {
        juce::AudioFormatManager formats;
        formats.registerBasicFormats();
        TimelineEngine engine;
        auto* timebase = new juce::DynamicObject();
        timebase->setProperty("ppq", 960);
        timebase->setProperty("bpm", 120.0);
        timebase->setProperty("timeSignatureNumerator", 4);
        timebase->setProperty("timeSignatureDenominator", 4);
        auto* loopRange = new juce::DynamicObject();
        loopRange->setProperty("enabled", false);
        loopRange->setProperty("startTick", 0);
        loopRange->setProperty("endTick", 0);
        juce::Array<juce::var> clips;
        const auto addClip = [&clips](
            const juce::String& id,
            const juce::File& file,
            const int sampleRate,
            const int frames) {
            auto* clip = new juce::DynamicObject();
            clip->setProperty("clipId", id);
            clip->setProperty("path", file.getFullPathName());
            clip->setProperty("sourceSampleRate", sampleRate);
            clip->setProperty("sourceStartFrame", 0);
            clip->setProperty("sourceEndFrame", frames);
            clip->setProperty("durationFrames", frames);
            clip->setProperty("durationSampleRate", sampleRate);
            clip->setProperty("startTick", 0);
            clip->setProperty("fadeInFrames", 0);
            clip->setProperty("fadeOutFrames", 0);
            clip->setProperty("gainDb", 0.0);
            clip->setProperty("pan", 0.0);
            clip->setProperty("loopEnabled", false);
            clip->setProperty("muted", false);
            clips.add(juce::var(clip));
        };
        addClip("mono-44100", mono, 44100, 44100);
        addClip("stereo-48000", stereo, 48000, 48000);
        auto* audioTrack = new juce::DynamicObject();
        audioTrack->setProperty("id", "track:test");
        audioTrack->setProperty("gainDb", 0.0);
        audioTrack->setProperty("pan", 0.0);
        audioTrack->setProperty("muted", false);
        audioTrack->setProperty("solo", false);
        audioTrack->setProperty("armed", true);
        audioTrack->setProperty("monitoring", "off");
        auto* audioInput = new juce::DynamicObject();
        audioInput->setProperty("channelIndex", 0);
        audioTrack->setProperty("audioInput", juce::var(audioInput));
        auto* rack = new juce::DynamicObject();
        rack->setProperty("devices", juce::Array<juce::var> {});
        audioTrack->setProperty("rack", juce::var(rack));
        audioTrack->setProperty("audioClips", clips);
        audioTrack->setProperty("midiClips", juce::Array<juce::var> {});
        juce::Array<juce::var> automationPoints;
        const auto addAutomationPoint = [&automationPoints](
                                            const juce::String& id,
                                            const int tick,
                                            const double value) {
            auto* point = new juce::DynamicObject();
            point->setProperty("id", id);
            point->setProperty("tick", tick);
            point->setProperty("value", value);
            automationPoints.add(juce::var(point));
        };
        addAutomationPoint("point:left", 0, -1.0);
        addAutomationPoint("point:right", 20, 1.0);
        auto* panAutomation = new juce::DynamicObject();
        panAutomation->setProperty("parameter", "pan");
        panAutomation->setProperty("points", automationPoints);
        juce::Array<juce::var> volumePoints;
        auto* quietPoint = new juce::DynamicObject();
        quietPoint->setProperty("id", "point:quiet");
        quietPoint->setProperty("tick", 0);
        quietPoint->setProperty("value", -24.0);
        volumePoints.add(juce::var(quietPoint));
        auto* loudPoint = new juce::DynamicObject();
        loudPoint->setProperty("id", "point:loud");
        loudPoint->setProperty("tick", 20);
        loudPoint->setProperty("value", 0.0);
        volumePoints.add(juce::var(loudPoint));
        auto* volumeAutomation = new juce::DynamicObject();
        volumeAutomation->setProperty("parameter", "volume");
        volumeAutomation->setProperty("points", volumePoints);
        juce::Array<juce::var> automation;
        automation.add(juce::var(volumeAutomation));
        automation.add(juce::var(panAutomation));
        audioTrack->setProperty("automation", automation);
        juce::Array<juce::var> tracks;
        tracks.add(juce::var(audioTrack));
        auto* placeholderTrack = new juce::DynamicObject();
        placeholderTrack->setProperty("id", "track:missing-instrument");
        placeholderTrack->setProperty("kind", "instrument");
        placeholderTrack->setProperty("gainDb", 0.0);
        placeholderTrack->setProperty("pan", 0.0);
        placeholderTrack->setProperty("muted", false);
        placeholderTrack->setProperty("solo", false);
        auto* placeholderInstrument = new juce::DynamicObject();
        placeholderInstrument->setProperty("id", "device:missing-instrument");
        placeholderInstrument->setProperty("path", "C:\\missing\\Instrument.vst3");
        placeholderInstrument->setProperty("disabledPlaceholder", true);
        placeholderTrack->setProperty(
            "instrument", juce::var(placeholderInstrument));
        auto* placeholderRack = new juce::DynamicObject();
        placeholderRack->setProperty("devices", juce::Array<juce::var> {});
        placeholderTrack->setProperty("rack", juce::var(placeholderRack));
        placeholderTrack->setProperty(
            "audioClips", juce::Array<juce::var> {});
        placeholderTrack->setProperty(
            "midiClips", juce::Array<juce::var> {});
        placeholderTrack->setProperty(
            "automation", juce::Array<juce::var> {});
        tracks.add(juce::var(placeholderTrack));
        auto* snapshotObject = new juce::DynamicObject();
        snapshotObject->setProperty("revision", 7);
        snapshotObject->setProperty("timebase", juce::var(timebase));
        snapshotObject->setProperty("loopRange", juce::var(loopRange));
        snapshotObject->setProperty("tracks", tracks);
        const auto snapshot = juce::var(snapshotObject);
        loaded = engine.loadSnapshot(snapshot, formats, 48000.0, 512, error);
        if (loaded) {
            snapshotObject->setProperty("revision", 8);
            audioTrack->setProperty("gainDb", -3.0);
            graphUpdateReusedDevices =
                engine.loadSnapshot(snapshot, formats, 48000.0, 512, error, false)
                && engine.preparedTrackReusesRuntimeDevices("track:test")
                && engine.commitPreparedSnapshot(error);
            OfflineRenderer offlineRenderer;
            OfflineRenderer::Result offlineResult;
            const auto offlineOutput = directory.getChildFile("offline-selection.wav");
            if (offlineRenderer.render(
                    snapshot,
                    formats,
                    offlineOutput,
                    480,
                    1440,
                    48000.0,
                    512,
                    0.0f,
                    false,
                    offlineResult,
                    error)) {
                auto reader = std::unique_ptr<juce::AudioFormatReader>(
                    formats.createReaderFor(offlineOutput));
                juce::AudioBuffer<float> rendered(2, 24000);
                offlineRangeRendered = reader != nullptr
                    && reader->numChannels == 2
                    && reader->lengthInSamples == 24000;
                offlineAudioRendered = offlineRangeRendered
                    && reader->read(&rendered, 0, 24000, 0, true, true)
                    && std::max(
                           rendered.getMagnitude(0, 0, 24000),
                           rendered.getMagnitude(1, 0, 24000))
                        > 0.01f;
            }
            engine.seekToTick(0);
            engine.play();
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            std::array<float, 512> left {};
            std::array<float, 512> right {};
            std::array<float*, 2> channels { left.data(), right.data() };
            engine.mix(channels.data(), 2, static_cast<int>(left.size()));
            automationEarlyLeft = std::abs(left[20]);
            automationEarlyRight = std::abs(right[20]);
            automationLateLeft = std::abs(left[490]);
            automationLateRight = std::abs(right[490]);
            automationRamped = std::abs(left[20]) > std::abs(right[20]) * 2.0f
                && std::abs(right[490]) > std::abs(left[490]) * 2.0f
                && std::abs(left[490]) + std::abs(right[490])
                    > (std::abs(left[20]) + std::abs(right[20])) * 4.0f;
            for (int block = 1; block < 8; ++block)
                engine.mix(channels.data(), 2, static_cast<int>(left.size()));
            const auto peak = std::max(
                *std::max_element(left.begin(), left.end()),
                *std::max_element(right.begin(), right.end()));
            mixed = peak > 0.1f;
            engine.seekToTick(960);
            const auto seekStatus = engine.status();
            seeked = static_cast<juce::int64>(seekStatus.getProperty("timelineSample", -1)) == 24000;

            CaptureIsolationSink captureSink(directory);
            engine.setRecordingSink(&captureSink);
            engine.seekToTick(0);
            int captureOffset = 0;
            int captureSamples = 0;
            std::array<float, 512> physicalInput {};
            physicalInput.fill(0.05f);
            std::array<float, 512> captureLeft {};
            std::array<float, 512> captureRight {};
            const std::array<const float*, 1> physicalInputs {
                physicalInput.data()
            };
            const std::array<float*, 2> captureOutputs {
                captureLeft.data(), captureRight.data()
            };
            const auto captureStarted = engine.startRecording(0, error);
            const auto captureWindow = captureStarted
                && engine.recordingWindow(
                    static_cast<int>(physicalInput.size()),
                    captureOffset,
                    captureSamples);
            if (captureWindow)
                engine.mix(
                    physicalInputs.data(),
                    1,
                    captureOutputs.data(),
                    2,
                    static_cast<int>(physicalInput.size()));
            engine.stopRecording();
            engine.stop();
            engine.clearRecordingSink();
            recordingTapIsolated = captureWindow
                && captureOffset == 0
                && captureSamples == static_cast<int>(physicalInput.size())
                && captureSink.receivedTrack == "track:test"
                && captureSink.receivedSamples == static_cast<int>(physicalInput.size())
                && captureSink.isolated;

            auto* loopSnapshot = new juce::DynamicObject();
            auto* loopTimebase = new juce::DynamicObject();
            loopTimebase->setProperty("ppq", 960);
            loopTimebase->setProperty("bpm", 120.0);
            loopTimebase->setProperty("timeSignatureNumerator", 4);
            loopTimebase->setProperty("timeSignatureDenominator", 4);
            auto* enabledLoop = new juce::DynamicObject();
            enabledLoop->setProperty("enabled", true);
            enabledLoop->setProperty("startTick", 0);
            enabledLoop->setProperty("endTick", 960);
            auto* punchRange = new juce::DynamicObject();
            punchRange->setProperty("startTick", 480);
            punchRange->setProperty("endTick", 960);
            loopSnapshot->setProperty("revision", 8);
            loopSnapshot->setProperty("timebase", juce::var(loopTimebase));
            loopSnapshot->setProperty("loopRange", juce::var(enabledLoop));
            loopSnapshot->setProperty("metronomeEnabled", true);
            auto* loopTrack = new juce::DynamicObject();
            loopTrack->setProperty("id", "track:loop");
            loopTrack->setProperty("gainDb", 0.0);
            loopTrack->setProperty("pan", 0.0);
            loopTrack->setProperty("muted", false);
            loopTrack->setProperty("solo", false);
            loopTrack->setProperty("armed", true);
            loopTrack->setProperty("monitoring", "off");
            auto* loopInput = new juce::DynamicObject();
            loopInput->setProperty("channelIndex", 0);
            loopTrack->setProperty("audioInput", juce::var(loopInput));
            auto* loopRack = new juce::DynamicObject();
            loopRack->setProperty("devices", juce::Array<juce::var> {});
            loopTrack->setProperty("rack", juce::var(loopRack));
            loopTrack->setProperty("audioClips", juce::Array<juce::var> {});
            loopTrack->setProperty("midiClips", juce::Array<juce::var> {});
            juce::Array<juce::var> loopTracks;
            loopTracks.add(juce::var(loopTrack));
            loopSnapshot->setProperty("tracks", loopTracks);
            const auto loopSnapshotValue = juce::var(loopSnapshot);
            auto punchSnapshot = juce::JSON::parse(
                juce::JSON::toString(loopSnapshotValue, false));
            punchSnapshot.getDynamicObject()->setProperty("punchRange", juce::var(punchRange));
            const auto loopSnapshotLoaded = engine.loadSnapshot(
                loopSnapshotValue, formats, 48000.0, 512, error);
            if (loopSnapshotLoaded) {
                CaptureIsolationSink loopCaptureSink(directory);
                engine.setRecordingSink(&loopCaptureSink);
                engine.seekToTick(0);
                int loopCaptureOffset = 0;
                int loopCaptureSamples = 0;
                constexpr int loopPassSamples = 24'000;
                constexpr int loopBlockSamples = 512;
                constexpr int loopTotalSamples = loopPassSamples * 3;
                std::array<float, loopBlockSamples> loopAudioInput {};
                loopAudioInput.fill(0.05f);
                std::array<float, loopBlockSamples> loopOutputLeft {};
                std::array<float, loopBlockSamples> loopOutputRight {};
                const std::array<const float*, 1> loopInputs { loopAudioInput.data() };
                const std::array<float*, 2> loopOutputs {
                    loopOutputLeft.data(), loopOutputRight.data()
                };
                const auto loopRecordingStarted = engine.startRecording(0, error);
                const auto loopWindowed = loopRecordingStarted
                    && engine.recordingWindow(
                        loopTotalSamples, loopCaptureOffset, loopCaptureSamples);
                auto loopRemaining = loopTotalSamples;
                while (loopWindowed && loopRemaining > loopBlockSamples) {
                    engine.mix(
                        loopInputs.data(),
                        1,
                        loopOutputs.data(),
                        2,
                        loopBlockSamples);
                    loopRemaining -= loopBlockSamples;
                }
                if (loopWindowed && loopRemaining > 0)
                    engine.mix(
                        loopInputs.data(),
                        1,
                        loopOutputs.data(),
                        2,
                        loopRemaining);
                engine.stopRecording();
                engine.flushRecordingTail(error);
                engine.stop();
                engine.clearRecordingSink();
                loopCaptureSegments = loopWindowed
                    && loopCaptureOffset == 0
                    && loopCaptureSamples == loopTotalSamples
                    && loopCaptureSink.beginCount == 3
                    && loopCaptureSink.endCount == 3
                    && loopCaptureSink.loopBoundaryCount == 3
                    && loopCaptureSink.totalRawSamples == loopTotalSamples
                    && loopCaptureSink.totalProcessedSamples == loopTotalSamples
                    && loopCaptureSink.offlineProcessedWriteCalls > 1
                    && loopCaptureSink.maxOfflineProcessedWriteSize <= loopBlockSamples;
                for (int index = 0; loopCaptureSegments && index < 3; ++index) {
                    const auto offset = static_cast<std::size_t>(index);
                    loopCaptureSegments = loopCaptureSink.segmentRawSamples[offset] > 0
                        && loopCaptureSink.endAudioSamples[offset]
                            > loopCaptureSink.beginAudioSamples[offset]
                        && (index == 0
                            || loopCaptureSink.beginAudioSamples[offset]
                                >= loopCaptureSink.endAudioSamples[offset - 1])
                        && loopCaptureSink.beginTimelineSamples[offset] == 0
                        && loopCaptureSink.endTimelineSamples[offset]
                            == static_cast<std::uint64_t>(loopPassSamples);
                }
            }
            // Synthetic Plugin Loop Recording test:
            // Latency 256, Tail 48000, Loop 24000, 3 passes with distinct impulses
            if (loopSnapshotLoaded && engine.loadSnapshot(
                    loopSnapshotValue, formats, 48000.0, 512, error)) {
                constexpr int kSynthDelay = 256;
                constexpr int kSynthTail = 48'000;
                constexpr int kSynthLoopLength = 24'000;
                constexpr int kSynthPasses = 3;
                constexpr int kSynthTotal = kSynthLoopLength * kSynthPasses;
                constexpr int kSynthBlock = 512;
                constexpr float kImpulseAmplitude = 0.9f;
                // Impulse positions within each pass (must be >= kSynthDelay)
                constexpr int kImpulsePos[kSynthPasses] = { 256, 1256, 2256 };
                // Set synthetic plugin delay and tail on the armed track
                {
                    const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                    if (engine.timeline != nullptr) {
                        for (auto& trackPtr : engine.timeline->tracks) {
                            if (!trackPtr->instrument && trackPtr->armed) {
                                trackPtr->pluginDelaySamples = kSynthDelay;
                                trackPtr->pluginTailSamples = kSynthTail;
                            }
                        }
                    }
                }
                LoopDataCaptureSink synthSink(directory, "synth");
                engine.setRecordingSink(&synthSink);
                engine.seekToTick(0);
                int synthOffset = 0;
                int synthSamples = 0;
                const auto synthStarted = engine.startRecording(0, error);
                const auto synthWindowed = synthStarted
                    && engine.recordingWindow(kSynthTotal, synthOffset, synthSamples);
                const auto clockBefore = engine.audioClockSample.load(
                    std::memory_order_acquire);
                std::array<float, kSynthBlock> synthInput {};
                std::array<float, kSynthBlock> synthOutL {};
                std::array<float, kSynthBlock> synthOutR {};
                const std::array<const float*, 1> synthInputs { synthInput.data() };
                const std::array<float*, 2> synthOutputs {
                    synthOutL.data(), synthOutR.data()
                };
                int synthMixed = 0;
                bool synthClockContinuous = true;
                while (synthWindowed && synthMixed < kSynthTotal) {
                    const auto block = std::min(kSynthBlock, kSynthTotal - synthMixed);
                    const auto passIndex = synthMixed / kSynthLoopLength;
                    const auto posInPass = synthMixed % kSynthLoopLength;
                    synthInput.fill(0.0f);
                    if (passIndex < kSynthPasses
                        && posInPass <= kImpulsePos[passIndex]
                        && kImpulsePos[passIndex] < posInPass + block) {
                        synthInput[static_cast<std::size_t>(
                            kImpulsePos[passIndex] - posInPass)] = kImpulseAmplitude;
                    }
                    const auto prevClock = engine.audioClockSample.load(
                        std::memory_order_acquire);
                    engine.mix(
                        synthInputs.data(), 1,
                        synthOutputs.data(), 2, block);
                    const auto newClock = engine.audioClockSample.load(
                        std::memory_order_acquire);
                    if (newClock != prevClock + static_cast<std::uint64_t>(block))
                        synthClockContinuous = false;
                    synthMixed += block;
                }
                engine.stopRecording();
                engine.flushRecordingTail(error);
                engine.stop();
                engine.clearRecordingSink();
                const auto clockAfter = engine.audioClockSample.load(
                    std::memory_order_acquire);
                // Verify: audio clock advanced continuously by total mixed samples
                const bool synthClockOk = synthClockContinuous
                    && clockAfter == clockBefore
                        + static_cast<std::uint64_t>(synthMixed);
                // Verify: timeline wrapped (position < loopLength after stop)
                const auto finalPosition = engine.timelineSample.load(
                    std::memory_order_acquire);
                const bool synthTimelineWrapped = finalPosition >= 0
                    && finalPosition < kSynthLoopLength;
                // Verify: 3 segments, 3 boundaries
                const bool synthSegmentsOk = synthSink.segmentCount == kSynthPasses
                    && synthSink.boundaryCount == kSynthPasses;
                // Verify: raw and processed lengths match
                const bool synthLengthsOk =
                    synthSink.totalRaw == kSynthTotal
                    && synthSink.totalProcessed == kSynthTotal
                    && static_cast<int>(synthSink.rawBuffer.size()) == kSynthTotal
                    && static_cast<int>(synthSink.processedLeft.size()) == kSynthTotal;
                // Verify: impulse positions and no cross-pass contamination
                bool synthImpulseOk = true;
                bool synthNoCrossPass = true;
                for (int pass = 0; synthImpulseOk && pass < kSynthPasses; ++pass) {
                    const auto base = static_cast<std::size_t>(pass * kSynthLoopLength);
                    const auto impulseAt = static_cast<std::size_t>(kImpulsePos[pass]);
                    // Raw impulse present at position P
                    synthImpulseOk = base + impulseAt < synthSink.rawBuffer.size()
                        && std::abs(synthSink.rawBuffer[base + impulseAt]
                            - kImpulseAmplitude) < 0.001f;
                    // Processed impulse at P - delay (passthrough chain + delay compensation)
                    // For a real plugin with latency D, output aligns at P.
                    // For passthrough test chain, compensation shifts left by D.
                    const auto processedPos = impulseAt
                        - static_cast<std::size_t>(kSynthDelay);
                    synthImpulseOk = synthImpulseOk
                        && base + processedPos < synthSink.processedLeft.size()
                        && std::abs(synthSink.processedLeft[base + processedPos]
                            - kImpulseAmplitude) < 0.001f;
                    // No other significant samples in this pass (no contamination)
                    for (int i = 0; synthNoCrossPass && i < kSynthLoopLength; ++i) {
                        if (static_cast<std::size_t>(i) == processedPos)
                            continue;
                        const auto idx = base + static_cast<std::size_t>(i);
                        if (idx < synthSink.processedLeft.size()
                            && std::abs(synthSink.processedLeft[idx]) > 0.001f)
                            synthNoCrossPass = false;
                    }
                }
                syntheticLoopPassed = synthWindowed && synthClockOk
                    && synthTimelineWrapped && synthSegmentsOk && synthLengthsOk
                    && synthImpulseOk && synthNoCrossPass
                    && synthSink.offlineProcessedWriteCalls > 1
                    && synthSink.maxOfflineProcessedWriteSize <= kSynthBlock;
            }
            // Partial Pass test: start recording from loop middle
            if (loopSnapshotLoaded && engine.loadSnapshot(
                    loopSnapshotValue, formats, 48000.0, 512, error)) {
                constexpr int kPartialTotal = 60'000; // 12000 + 24000 + 24000
                constexpr int kPartialBlock = 512;
                // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                {
                    const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                    if (engine.timeline != nullptr) {
                        for (auto& trackPtr : engine.timeline->tracks) {
                            if (!trackPtr->instrument && trackPtr->armed) {
                                trackPtr->pluginDelaySamples = 0;
                                trackPtr->pluginTailSamples = 0;
                            }
                        }
                    }
                }
                LoopDataCaptureSink partialSink(directory, "partial");
                engine.setRecordingSink(&partialSink);
                engine.seekToTick(480); // tick 480 = sample 12000
                int partialOffset = 0;
                int partialSamples = 0;
                const auto partialStarted = engine.startRecording(0, error);
                const auto partialWindowed = partialStarted
                    && engine.recordingWindow(
                        kPartialTotal, partialOffset, partialSamples);
                std::array<float, kPartialBlock> partialInput {};
                partialInput.fill(0.05f);
                std::array<float, kPartialBlock> partialOutL {};
                std::array<float, kPartialBlock> partialOutR {};
                const std::array<const float*, 1> partialInputs { partialInput.data() };
                const std::array<float*, 2> partialOutputs {
                    partialOutL.data(), partialOutR.data()
                };
                int partialMixed = 0;
                while (partialWindowed && partialMixed < kPartialTotal) {
                    const auto block = std::min(kPartialBlock, kPartialTotal - partialMixed);
                    engine.mix(
                        partialInputs.data(), 1,
                        partialOutputs.data(), 2, block);
                    partialMixed += block;
                }
                engine.stopRecording();
                engine.flushRecordingTail(error);
                engine.stop();
                engine.clearRecordingSink();
                // Expected: 3 segments (partial 12000, full 24000, full 24000)
                const bool partialSegmentsOk = partialSink.segmentCount == 3;
                const bool partialLengthsOk =
                    partialSink.totalRaw == kPartialTotal
                    && partialSink.totalProcessed == kPartialTotal
                    && static_cast<int>(partialSink.processedLeft.size()) == kPartialTotal;
                // Each segment independently reset (no cross-segment contamination)
                // With constant 0.05 input and passthrough chain, all processed ≈ 0.05
                bool partialDataOk = partialSink.processedLeft.size()
                    == static_cast<std::size_t>(kPartialTotal);
                int partialFailIndex = -1;
                for (int i = 0; partialDataOk && i < kPartialTotal; ++i) {
                    if (std::abs(partialSink.processedLeft[
                            static_cast<std::size_t>(i)] - 0.05f) >= 0.02f) {
                        partialDataOk = false;
                        partialFailIndex = i;
                    }
                }
                partialPassPassed = partialWindowed && partialSegmentsOk
                    && partialLengthsOk && partialDataOk
                    && partialSink.offlineProcessedWriteCalls > 1
                    && partialSink.maxOfflineProcessedWriteSize <= kPartialBlock;
                diagPartialSegments = partialSink.segmentCount;
                diagPartialRaw = partialSink.totalRaw;
                diagPartialProcessed = partialSink.totalProcessed;
                diagPartialWindowed = partialWindowed ? 1 : 0;
                diagPartialFailIndex = partialFailIndex;
                if (partialFailIndex >= 0
                    && static_cast<std::size_t>(partialFailIndex)
                        < partialSink.processedLeft.size())
                    diagPartialFailValue = partialSink.processedLeft[
                        static_cast<std::size_t>(partialFailIndex)];
                if (partialFailIndex >= 0
                    && static_cast<std::size_t>(partialFailIndex)
                        < partialSink.rawBuffer.size())
                    diagPartialRawAtFail = partialSink.rawBuffer[
                        static_cast<std::size_t>(partialFailIndex)];
            }
            // Block Size test: verify processing uses small blocks
            if (loopSnapshotLoaded && engine.loadSnapshot(
                    loopSnapshotValue, formats, 48000.0, 128, error)) {
                // preparedBlockSize = 128; chain must be fed in <= 128 sample chunks
                constexpr int kBsTotal = 24'000; // 1 pass
                constexpr int kBsBlock = 128;
                constexpr int kBsDelay = 64;
                constexpr float kBsImpulse = 0.8f;
                constexpr int kBsImpulsePos = 500;
                {
                    const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                    if (engine.timeline != nullptr) {
                        for (auto& trackPtr : engine.timeline->tracks) {
                            if (!trackPtr->instrument && trackPtr->armed)
                                trackPtr->pluginDelaySamples = kBsDelay;
                        }
                    }
                }
                LoopDataCaptureSink bsSink(directory, "blocksize");
                engine.setRecordingSink(&bsSink);
                engine.seekToTick(0);
                int bsOffset = 0;
                int bsSamples = 0;
                const auto bsStarted = engine.startRecording(0, error);
                const auto bsWindowed = bsStarted
                    && engine.recordingWindow(kBsTotal, bsOffset, bsSamples);
                std::array<float, kBsBlock> bsInput {};
                std::array<float, kBsBlock> bsOutL {};
                std::array<float, kBsBlock> bsOutR {};
                const std::array<const float*, 1> bsInputs { bsInput.data() };
                const std::array<float*, 2> bsOutputs {
                    bsOutL.data(), bsOutR.data()
                };
                int bsMixed = 0;
                while (bsWindowed && bsMixed < kBsTotal) {
                    bsInput.fill(0.0f);
                    if (bsMixed <= kBsImpulsePos
                        && kBsImpulsePos < bsMixed + kBsBlock) {
                        bsInput[static_cast<std::size_t>(
                            kBsImpulsePos - bsMixed)] = kBsImpulse;
                    }
                    engine.mix(
                        bsInputs.data(), 1,
                        bsOutputs.data(), 2, kBsBlock);
                    bsMixed += kBsBlock;
                }
                engine.stopRecording();
                engine.flushRecordingTail(error);
                engine.stop();
                engine.clearRecordingSink();
                // Verify: processed length matches raw, impulse at correct position
                const bool bsLengthOk = bsSink.totalRaw == kBsTotal
                    && bsSink.totalProcessed == kBsTotal;
                const auto bsProcessedPos = kBsImpulsePos - kBsDelay;
                const bool bsImpulseOk = bsProcessedPos >= 0
                    && static_cast<std::size_t>(bsProcessedPos)
                        < bsSink.processedLeft.size()
                    && std::abs(bsSink.processedLeft[
                        static_cast<std::size_t>(bsProcessedPos)]
                        - kBsImpulse) < 0.01f;
                blockSizePassed = bsWindowed && bsLengthOk && bsImpulseOk
                    && bsSink.offlineProcessedWriteCalls > 1
                    && bsSink.maxOfflineProcessedWriteSize <= kBsBlock;
                diagBsRaw = bsSink.totalRaw;
                diagBsProcessed = bsSink.totalProcessed;
                diagBsWindowed = bsWindowed ? 1 : 0;
            }
            // Long Recording test: 130 passes exceeds old 128x RAM limit
            if (loopSnapshotLoaded && engine.loadSnapshot(
                    loopSnapshotValue, formats, 48000.0, 512, error)) {
                constexpr int kLongLoopLength = 4'800; // short loop
                constexpr int kLongPasses = 130;
                constexpr int kLongTotal = kLongLoopLength * kLongPasses;
                constexpr int kLongBlock = 512;
                // Reset delay (may have been set by a previous test with reuseRuntimeDevices)
                {
                    const juce::SpinLock::ScopedLockType lock(engine.timelineLock);
                    if (engine.timeline != nullptr) {
                        for (auto& trackPtr : engine.timeline->tracks) {
                            if (!trackPtr->instrument && trackPtr->armed) {
                                trackPtr->pluginDelaySamples = 0;
                                trackPtr->pluginTailSamples = 0;
                            }
                        }
                    }
                }
                // Use a snapshot with short loop (tick 0..192 = 4800 samples)
                auto* longLoop = new juce::DynamicObject();
                longLoop->setProperty("enabled", true);
                longLoop->setProperty("startTick", 0);
                longLoop->setProperty("endTick", 192);
                loopSnapshot->setProperty("loopRange", juce::var(longLoop));
                const auto longSnapshotValue = juce::var(loopSnapshot);
                if (engine.loadSnapshot(
                        longSnapshotValue, formats, 48000.0, 512, error)) {
                    LoopDataCaptureSink longSink(directory, "longrec");
                    engine.setRecordingSink(&longSink);
                    engine.seekToTick(0);
                    int longOffset = 0;
                    int longSamples = 0;
                    const auto longStarted = engine.startRecording(0, error);
                    const auto longWindowed = longStarted
                        && engine.recordingWindow(
                            kLongTotal, longOffset, longSamples);
                    std::array<float, kLongBlock> longInput {};
                    longInput.fill(0.02f);
                    std::array<float, kLongBlock> longOutL {};
                    std::array<float, kLongBlock> longOutR {};
                    const std::array<const float*, 1> longInputs { longInput.data() };
                    const std::array<float*, 2> longOutputs {
                        longOutL.data(), longOutR.data()
                    };
                    int longMixed = 0;
                    while (longWindowed && longMixed < kLongTotal) {
                        const auto block = std::min(kLongBlock, kLongTotal - longMixed);
                        engine.mix(
                            longInputs.data(), 1,
                            longOutputs.data(), 2, block);
                        longMixed += block;
                    }
                    engine.stopRecording();
                    engine.flushRecordingTail(error);
                    engine.stop();
                    engine.clearRecordingSink();
                    // Verify: all 130 passes recorded, raw/processed match
                    longRecordingPassed = longWindowed
                        && longSink.segmentCount == kLongPasses
                        && longSink.totalRaw == kLongTotal
                        && longSink.totalProcessed == kLongTotal
                        && static_cast<int>(longSink.processedLeft.size())
                            == kLongTotal
                        && longSink.offlineProcessedWriteCalls > 1
                        && longSink.maxOfflineProcessedWriteSize <= kLongBlock;
                }
                // Restore original loop range for subsequent tests
                auto* restoreLoop = new juce::DynamicObject();
                restoreLoop->setProperty("enabled", true);
                restoreLoop->setProperty("startTick", 0);
                restoreLoop->setProperty("endTick", 960);
                loopSnapshot->setProperty("loopRange", juce::var(restoreLoop));
            }
            // Production Writer Integration Test: 4 bars x 3 passes (384,000 x 3)
            {
                auto* prodTimebase = new juce::DynamicObject();
                prodTimebase->setProperty("ppq", 960);
                prodTimebase->setProperty("bpm", 120.0);
                prodTimebase->setProperty("timeSignatureNumerator", 4);
                prodTimebase->setProperty("timeSignatureDenominator", 4);
                auto* prodLoop = new juce::DynamicObject();
                prodLoop->setProperty("enabled", true);
                prodLoop->setProperty("startTick", 0);
                prodLoop->setProperty("endTick", 15360);
                auto* prodTrack = new juce::DynamicObject();
                prodTrack->setProperty("id", "track:prod");
                prodTrack->setProperty("gainDb", 0.0);
                prodTrack->setProperty("pan", 0.0);
                prodTrack->setProperty("muted", false);
                prodTrack->setProperty("solo", false);
                prodTrack->setProperty("armed", true);
                prodTrack->setProperty("monitoring", "off");
                auto* prodInput = new juce::DynamicObject();
                prodInput->setProperty("channelIndex", 0);
                prodTrack->setProperty("audioInput", juce::var(prodInput));
                auto* prodRack = new juce::DynamicObject();
                prodRack->setProperty("devices", juce::Array<juce::var> {});
                prodTrack->setProperty("rack", juce::var(prodRack));
                prodTrack->setProperty("audioClips", juce::Array<juce::var> {});
                prodTrack->setProperty("midiClips", juce::Array<juce::var> {});
                juce::Array<juce::var> prodTracks;
                prodTracks.add(juce::var(prodTrack));
                auto* prodSnapshot = new juce::DynamicObject();
                prodSnapshot->setProperty("revision", 10);
                prodSnapshot->setProperty("timebase", juce::var(prodTimebase));
                prodSnapshot->setProperty("loopRange", juce::var(prodLoop));
                prodSnapshot->setProperty("tracks", prodTracks);
                const auto prodSnapshotValue = juce::var(prodSnapshot);

                if (engine.loadSnapshot(
                        prodSnapshotValue, formats, 48000.0, 512, error)) {
                    constexpr int kProdLoopLength = 384'000;
                    constexpr int kProdPasses = 3;
                    constexpr int kProdTotal = kProdLoopLength * kProdPasses;
                    constexpr int kProdBlock = 512;

                    // 3 full passes
                    // SafetyAudioCallback owns stop, tail flush, sink clear and session finish.
                    engine.seekToTick(0);
                    auto prodDir = directory.getChildFile("prod-writer");
                    SafetyAudioCallback prodCallback;
                    prodCallback.setTimelineEngine(&engine);
                    juce::String sessionError;
                    const auto prodArrangeStarted =
                        prodCallback.startArrangeRecording(
                            prodDir, engine, sessionError);
                    if (prodArrangeStarted) {
                        int prodOffset = 0;
                        int prodSamples = 0;
                        const auto prodStarted = engine.startRecording(0, error);
                        const auto prodWindowed = prodStarted
                            && engine.recordingWindow(
                                kProdTotal, prodOffset, prodSamples);
                        std::array<float, kProdBlock> prodIn {};
                        prodIn.fill(0.06f);
                        std::array<float, kProdBlock> prodOutL {};
                        std::array<float, kProdBlock> prodOutR {};
                        const std::array<const float*, 1> prodInputs {
                            prodIn.data()
                        };
                        const std::array<float*, 2> prodOutputs {
                            prodOutL.data(), prodOutR.data()
                        };
                        int prodMixed = 0;
                        while (prodWindowed && prodMixed < kProdTotal) {
                            const auto block = std::min(
                                kProdBlock, kProdTotal - prodMixed);
                            engine.mix(
                                prodInputs.data(), 1,
                                prodOutputs.data(), 2, block);
                            prodMixed += block;
                            juce::Thread::sleep(10);
                        }
                        const auto preStopStatus = prodCallback.recordingStatus();
                        juce::String stopError;
                        const auto stopOk =
                            prodCallback.stopArrangeRecording(engine, stopError);
                        engine.stop();

                        const auto rawFile =
                            prodDir.getChildFile("tracks/0000/raw.wav");
                        const auto processedFile =
                            prodDir.getChildFile("tracks/0000/processed.wav");
                        const auto manifestFile =
                            prodDir.getChildFile("manifest.json");
                        const auto manifestText =
                            manifestFile.loadFileAsString();
                        const auto manifestValue =
                            juce::JSON::parse(manifestText);
                        auto rawReader = std::unique_ptr<juce::AudioFormatReader>(
                            formats.createReaderFor(rawFile));
                        auto processedReader =
                            std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                        const auto rawLength = rawReader != nullptr
                            ? rawReader->lengthInSamples : 0;
                        const auto processedLength = processedReader != nullptr
                            ? processedReader->lengthInSamples : 0;
                        const auto completed = manifestText.contains(
                            "\"state\": \"completed\"");

                        diagProductionRawSamples =
                            static_cast<std::uint64_t>(rawLength);
                        diagProductionProcessedSamples =
                            static_cast<std::uint64_t>(processedLength);
                        if (preStopStatus.isObject()) {
                            diagProductionMissing = static_cast<std::uint64_t>(
                                static_cast<juce::int64>(preStopStatus.getProperty(
                                    "processedMissingSamples", 0)));
                            diagProductionDropped = static_cast<std::uint64_t>(
                                static_cast<juce::int64>(preStopStatus.getProperty(
                                    "droppedBlocks", 0)));
                        }

                        productionWriterPassed = prodWindowed && stopOk
                            && rawFile.existsAsFile()
                            && processedFile.existsAsFile()
                            && rawLength == kProdTotal
                            && processedLength == kProdTotal
                            && completed
                            && diagProductionMissing == 0
                            && diagProductionDropped == 0;

                        // Verify capture segment ranges match
                        if (productionWriterPassed && manifestValue.isObject()) {
                            const auto manifestTracks = manifestValue.getProperty(
                                "tracks", {});
                            if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                const auto trackObj = manifestTracks[0];
                                const auto segments = trackObj.getProperty(
                                    "captureSegments", {});
                                if (segments.isArray()
                                    && segments.size() == kProdPasses) {
                                    for (int i = 0; i < kProdPasses; ++i) {
                                        const auto seg = segments[i];
                                        const auto rawStart = static_cast<juce::int64>(
                                            seg.getProperty(
                                                "rawFileStartSample", -1));
                                        const auto rawEnd = static_cast<juce::int64>(
                                            seg.getProperty(
                                                "rawFileEndSample", -1));
                                        const auto procStart =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "processedFileStartSample",
                                                    -1));
                                        const auto procEnd =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "processedFileEndSample",
                                                    -1));
                                        if (rawStart != procStart
                                            || rawEnd != procEnd) {
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
                    engine.seekToTick(7680);
                    auto partialConfig = engine.recordingConfiguration();
                    juce::String partialSessionError;
                    auto partialDir =
                        directory.getChildFile("prod-writer-partial");
                    auto partialSession = ArrangeRecordingSession::create(
                        partialDir, partialConfig, partialSessionError);
                    if (partialSession != nullptr) {
                        engine.setRecordingSink(partialSession.get());
                        constexpr int kPartialTotal = 768'000;
                        int partialOffset = 0;
                        int partialSamples = 0;
                        const auto partialStarted = engine.startRecording(
                            0, error);
                        const auto partialWindowed = partialStarted
                            && engine.recordingWindow(
                                kPartialTotal, partialOffset, partialSamples);
                        std::array<float, kProdBlock> partIn {};
                        partIn.fill(0.06f);
                        std::array<float, kProdBlock> partOutL {};
                        std::array<float, kProdBlock> partOutR {};
                        const std::array<const float*, 1> partInputs {
                            partIn.data()
                        };
                        const std::array<float*, 2> partOutputs {
                            partOutL.data(), partOutR.data()
                        };
                        int partMixed = 0;
                        while (partialWindowed && partMixed < kPartialTotal) {
                            const auto block = std::min(
                                kProdBlock, kPartialTotal - partMixed);
                            engine.mix(
                                partInputs.data(), 1,
                                partOutputs.data(), 2, block);
                            partMixed += block;
                            juce::Thread::sleep(10);
                        }
                        engine.stopRecording();
                        const auto tailOk = engine.flushRecordingTail(error);
                        engine.stop();
                        engine.clearRecordingSink();
                        juce::String finishError;
                        const auto finished = partialSession->finish(
                            finishError);

                        const auto rawFile = partialDir.getChildFile(
                            "tracks/0000/raw.wav");
                        const auto processedFile = partialDir.getChildFile(
                            "tracks/0000/processed.wav");
                        const auto manifestFile = partialDir.getChildFile(
                            "manifest.json");
                        const auto manifestText =
                            manifestFile.loadFileAsString();
                        const auto manifestValue =
                            juce::JSON::parse(manifestText);
                        auto rawReader =
                            std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(rawFile));
                        auto processedReader =
                            std::unique_ptr<juce::AudioFormatReader>(
                                formats.createReaderFor(processedFile));

                        const auto rawLength = rawReader != nullptr
                            ? rawReader->lengthInSamples : 0;
                        const auto processedLength =
                            processedReader != nullptr
                            ? processedReader->lengthInSamples : 0;
                        const auto completed = manifestText.contains(
                            "\"state\": \"completed\"");

                        diagProductionPartialRaw =
                            static_cast<std::uint64_t>(rawLength);
                        diagProductionPartialProcessed =
                            static_cast<std::uint64_t>(processedLength);

                        productionWriterPartialPassed = partialWindowed
                            && tailOk && finished
                            && rawFile.existsAsFile()
                            && processedFile.existsAsFile()
                            && rawLength == kPartialTotal
                            && processedLength == kPartialTotal
                            && completed;

                        // Verify 3 segments and raw/processed ranges
                        if (productionWriterPartialPassed
                            && manifestValue.isObject()) {
                            const auto manifestTracks = manifestValue.getProperty(
                                "tracks", {});
                            if (manifestTracks.isArray() && manifestTracks.size() > 0) {
                                const auto trackObj = manifestTracks[0];
                                const auto segments = trackObj.getProperty(
                                    "captureSegments", {});
                                if (segments.isArray()
                                    && segments.size() == 3) {
                                    for (int i = 0; i < 3; ++i) {
                                        const auto seg = segments[i];
                                        const auto rawStart =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "rawFileStartSample",
                                                    -1));
                                        const auto rawEnd =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "rawFileEndSample",
                                                    -1));
                                        const auto procStart =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "processedFileStartSample",
                                                    -1));
                                        const auto procEnd =
                                            static_cast<juce::int64>(
                                                seg.getProperty(
                                                    "processedFileEndSample",
                                                    -1));
                                        if (rawStart != procStart
                                            || rawEnd != procEnd) {
                                            productionWriterPartialPassed =
                                                false;
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
            if (loopSnapshotLoaded && engine.loadSnapshot(
                    punchSnapshot, formats, 48000.0, 512, error)) {
                int punchOffset = 0;
                int punchSamples = 0;
                engine.seekToTick(480);
                error.clear();
                const auto punchStarted = engine.startRecording(0, error);
                punchWindowed = punchStarted && engine.recordingWindow(512, punchOffset, punchSamples)
                    && punchOffset == 0 && punchSamples == 512;
                if (!punchStarted && error.isEmpty())
                    error = "Punch self-test could not start Arrange recording.";
                engine.stopRecording();
                engine.stop();
                engine.seekToTick(480);
                if (engine.startRecording(0, error)) {
                    int immediateOffset = 0;
                    int immediateSamples = 0;
                    std::array<float, 512> immediateOutput {};
                    std::array<float*, 1> immediateChannels {
                        immediateOutput.data()
                    };
                    const auto immediateWindow = engine.recordingWindow(
                        static_cast<int>(immediateOutput.size()),
                        immediateOffset,
                        immediateSamples);
                    engine.mix(
                        immediateChannels.data(),
                        1,
                        static_cast<int>(immediateOutput.size()));
                    const auto immediateStatus = engine.status();
                    immediateRecordStarted = immediateWindow
                        && immediateOffset == 0
                        && immediateSamples == 512
                        && immediateStatus.getProperty("state", {}).toString() == "playing"
                        && static_cast<juce::int64>(
                            immediateStatus.getProperty("timelineSample", -1))
                            == 12'512;
                    engine.stopRecording();
                    engine.stop();
                    engine.seekToTick(480);
                }
                if (engine.startRecording(1, error)) {
                    int countInOffset = 0;
                    int countInSamples = 0;
                    constexpr int countInBlockSamples = 24'128;
                    const auto countInWindow = engine.recordingWindow(
                        countInBlockSamples, countInOffset, countInSamples);
                    std::vector<float> countInOutput(countInBlockSamples);
                    std::array<float*, 1> countInChannels { countInOutput.data() };
                    engine.mix(
                        countInChannels.data(),
                        1,
                        static_cast<int>(countInOutput.size()));
                    engine.mixMetronome(
                        countInChannels.data(),
                        1,
                        static_cast<int>(countInOutput.size()));
                    countInAligned = countInWindow
                        && countInOffset == 24'000
                        && countInSamples == 128
                        && static_cast<juce::int64>(
                            engine.status().getProperty("timelineSample", -1))
                            == 12'128;
                    countInAudible =
                        *std::max_element(countInOutput.begin(), countInOutput.end()) > 0.0f;
                    engine.stopRecording();
                }
                engine.stop();
                engine.seekToTick(480);
                if (engine.startRecording(2, error)) {
                    int cancelledOffset = 0;
                    int cancelledSamples = 0;
                    countInCancelled = engine.cancelRecordingIfCountingIn()
                        && engine.status().getProperty(
                            "recordingPhase", {}).toString() == "idle"
                        && !engine.recordingWindow(
                            512, cancelledOffset, cancelledSamples)
                        && cancelledSamples == 0;
                }
                engine.play();
                engine.seekToTick(0);
                std::array<float, 24000> silent {};
                std::array<float*, 1> silentChannels { silent.data() };
                engine.mix(silentChannels.data(), 1, static_cast<int>(silent.size()));
                looped = static_cast<juce::int64>(
                    engine.status().getProperty("timelineSample", -1)) == 0;
                std::array<float, 512> clicks {};
                std::array<float*, 1> clickChannels { clicks.data() };
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
    addCheck(
        "Offline Render writes the exact tick selection",
        offlineRangeRendered);
    addCheck(
        "Offline Render receives audio from the Arrangement Graph",
        offlineAudioRendered);
    addCheck(
        "mix edits swap the Graph without reloading Track Devices",
        graphUpdateReusedDevices);
    addCheck(
        "Parameter and Bypass changes do not alter Plugin Topology",
        mutablePluginStateKeepsTopology);
    addCheck(
        "recording taps exclude Timeline playback and Track mix gain",
        recordingTapIsolated);
    addCheck(
        "Timeline loop capture closes three non-overlapping Audio segments",
        loopCaptureSegments);
    addCheck(
        "Synthetic Plugin loop recording produces aligned Raw/Processed without stopping",
        syntheticLoopPassed);
    addCheck(
        "Partial Pass recording generates independent Processed segments",
        partialPassPassed);
    addCheck(
        "Block Size processing respects prepared block size limit",
        blockSizePassed);
    addCheck(
        "Long Recording (130 passes) matches Raw/Processed without RAM pre-allocation",
        longRecordingPassed);
    addCheck(
        "Production ThreadedWriter 4小節×3 Pass (SafetyAudioCallback owns stop, tail flush, sink clear and session finish)",
        productionWriterPassed);
    addCheck(
        "Production ThreadedWriter Partial Pass",
        productionWriterPartialPassed);
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
    result->setProperty("productionRawSamples", static_cast<juce::int64>(diagProductionRawSamples));
    result->setProperty("productionProcessedSamples", static_cast<juce::int64>(diagProductionProcessedSamples));
    result->setProperty("productionMissingSamples", static_cast<juce::int64>(diagProductionMissing));
    result->setProperty("productionDroppedBlocks", static_cast<juce::int64>(diagProductionDropped));
    result->setProperty("productionPartialRaw", static_cast<juce::int64>(diagProductionPartialRaw));
    result->setProperty("productionPartialProcessed", static_cast<juce::int64>(diagProductionPartialProcessed));
    result->setProperty(
        "passed", sourcesWritten && loaded && mixed && seeked && looped && punchWindowed
            && immediateRecordStarted && countInAligned && countInAudible && countInCancelled
            && metronomeMixed && automationRamped && offlineRangeRendered
            && offlineAudioRendered && graphUpdateReusedDevices
            && mutablePluginStateKeepsTopology && recordingTapIsolated
            && loopCaptureSegments && syntheticLoopPassed
            && partialPassPassed && blockSizePassed && longRecordingPassed
            && productionWriterPassed && productionWriterPartialPassed);
    mono.deleteFile();
    stereo.deleteFile();
    directory.getChildFile("offline-selection.wav").deleteFile();
    return juce::var(result);
}

} // namespace riffra
