#include "TimelineEngine.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <fstream>
#include <thread>

namespace riffra {

namespace {
constexpr int kReadAheadSamples = 32768;

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
} // namespace

TimelineEngine::TimelineEngine() { readAheadThread.startThread(); }

TimelineEngine::~TimelineEngine() {
    stop();
    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        timeline.reset();
    }
    readAheadThread.stopThread(3000);
}

std::int64_t TimelineEngine::tickToSample(
    const std::uint64_t tick,
    const std::uint32_t ppq,
    const double bpm,
    const double sampleRate) noexcept {
    if (ppq == 0 || bpm <= 0.0 || sampleRate <= 0.0) return 0;
    return static_cast<std::int64_t>(std::llround(
        static_cast<double>(tick) * sampleRate * 60.0 /
        (bpm * static_cast<double>(ppq))));
}

bool TimelineEngine::loadSnapshot(
    const juce::var& snapshot,
    juce::AudioFormatManager& formats,
    const double outputSampleRate,
    const int maximumBlockSize,
    juce::String& error) {
    if (!snapshot.isObject() || outputSampleRate <= 0.0 || maximumBlockSize <= 0) {
        error = "Timeline snapshot requires an active audio device.";
        return false;
    }
    auto prepared = std::make_unique<PreparedTimeline>();
    prepared->revision = static_cast<std::uint64_t>(
        static_cast<juce::int64>(snapshot.getProperty("revision", -1)));
    const auto timebase = snapshot.getProperty("timebase", {});
    double ppq = 0.0;
    if (!timebase.isObject() || !requiredNumber(timebase, "ppq", ppq) ||
        !requiredNumber(timebase, "bpm", prepared->bpm) || ppq != 960.0 ||
        prepared->bpm < 20.0 || prepared->bpm > 400.0) {
        error = "Timeline snapshot has an invalid timebase.";
        return false;
    }
    prepared->ppq = static_cast<std::uint32_t>(ppq);
    prepared->outputSampleRate = outputSampleRate;

    const auto loopRange = snapshot.getProperty("loopRange", {});
    if (loopRange.isObject()) {
        prepared->loopEnabled = static_cast<bool>(loopRange.getProperty("enabled", false));
        const auto startTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("startTick", 0)));
        const auto endTick = static_cast<std::uint64_t>(
            static_cast<juce::int64>(loopRange.getProperty("endTick", 0)));
        prepared->loopStartSample = tickToSample(
            startTick, prepared->ppq, prepared->bpm, outputSampleRate);
        prepared->loopEndSample = tickToSample(
            endTick, prepared->ppq, prepared->bpm, outputSampleRate);
        if (prepared->loopEnabled && prepared->loopEndSample <= prepared->loopStartSample) {
            error = "Timeline loop range must have a positive duration.";
            return false;
        }
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
        if (track->id.isEmpty()) {
            error = "Timeline track requires an id.";
            return false;
        }
        track->gain = juce::Decibels::decibelsToGain(
            static_cast<float>(trackValue.getProperty("gainDb", 0.0)));
        track->pan = juce::jlimit(
            -1.0f, 1.0f, static_cast<float>(trackValue.getProperty("pan", 0.0)));
        track->muted = static_cast<bool>(trackValue.getProperty("muted", false));
        track->solo = static_cast<bool>(trackValue.getProperty("solo", false));

        const auto rack = trackValue.getProperty("rack", {});
        if (rack.isObject()) {
            const auto devices = rack.getProperty("devices", {});
            if (!devices.isArray()) {
                error = "Timeline track rack devices must be an array.";
                return false;
            }
            for (const auto& device : *devices.getArray()) {
                if (!device.isObject() || device.getProperty("kind", {}).toString() != "plugin" ||
                    static_cast<bool>(device.getProperty("disabledPlaceholder", false)))
                    continue;
                if (track->rack != nullptr) {
                    error = "Timeline track racks support one active plugin per track.";
                    return false;
                }
                const auto path = device.getProperty("path", {}).toString();
                auto rackInstance = std::make_unique<PluginRack>();
                if (const auto loadError = rackInstance->load(path, outputSampleRate, maximumBlockSize)) {
                    error = "Track rack plugin could not be loaded: " + loadError->message;
                    return false;
                }
                const auto stateData = device.getProperty("stateData", {}).toString();
                if (stateData.isNotEmpty()) {
                    juce::String stateError;
                    if (!rackInstance->setState(stateData, stateError)) {
                        error = "Track rack plugin state could not be restored: " + stateError;
                        return false;
                    }
                } else {
                    const auto values = device.getProperty("parameterValues", {});
                    if (values.isArray()) {
                        const auto parameterStatus = rackInstance->parameterStatus();
                        const auto parameters = parameterStatus.getProperty("parameters", {});
                        const auto addressable = parameters.isArray() ? parameters.size() : 0;
                        for (int index = 0; index < std::min(values.size(), addressable); ++index) {
                            juce::String parameterError;
                            if (!rackInstance->setParameter(
                                    index, static_cast<float>(values[index]), parameterError)) {
                                error = "Track rack plugin parameter could not be restored: " +
                                    parameterError;
                                return false;
                            }
                        }
                    }
                }
                rackInstance->setBypassed(static_cast<bool>(device.getProperty("bypassed", false)));
                track->pluginDelaySamples = rackInstance->latencySamples();
                maximumPluginDelay = std::max(maximumPluginDelay, track->pluginDelaySamples);
                track->rack = std::move(rackInstance);
            }
        }

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
            clip->startSample = tickToSample(
                startTick, prepared->ppq, prepared->bpm, outputSampleRate);
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
                kReadAheadSamples,
                &readAheadThread,
                clip->sourceSampleRate,
                2);
            clip->transport.prepareToPlay(maximumBlockSize, outputSampleRate);
            clip->transport.start();
            clip->scratch.setSize(2, maximumBlockSize, false, true, false);
            track->clips.push_back(std::move(clip));
        }
        track->mixBuffer.setSize(2, maximumBlockSize, false, true, false);
        track->processedBuffer.setSize(2, maximumBlockSize, false, true, false);
        prepared->tracks.push_back(std::move(track));
    }
    for (auto& track : prepared->tracks) {
        track->compensationDelaySamples = maximumPluginDelay - track->pluginDelaySamples;
        track->delayBuffer.setSize(
            2, static_cast<int>(track->compensationDelaySamples + maximumBlockSize + 1),
            false, true, false);
        track->delayBuffer.clear();
    }

    {
        const juce::SpinLock::ScopedLockType lock(timelineLock);
        const auto hasExistingTimeline = timeline != nullptr;
        timeline = std::move(prepared);
        if (!hasExistingTimeline)
            timelineSample.store(0, std::memory_order_release);
    }
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void TimelineEngine::play() noexcept {
    state.store(State::playing, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::stop() noexcept {
    state.store(State::stopped, std::memory_order_release);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::audioDeviceStarted() noexcept {
    audioClockSample.store(0, std::memory_order_release);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr)
        resetTrackState(*timeline);
    clockGeneration.fetch_add(1, std::memory_order_relaxed);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::seekToTick(const std::uint64_t tick) noexcept {
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    timelineSample.store(
        tickToSample(tick, timeline->ppq, timeline->bpm, timeline->outputSampleRate),
        std::memory_order_release);
    resetTrackState(*timeline);
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
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

void TimelineEngine::processTracks(
    PreparedTimeline& prepared,
    float* const* outputChannels,
    const int channelCount,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto hasSolo = std::any_of(
        prepared.tracks.begin(), prepared.tracks.end(),
        [](const auto& track) { return track->solo; });
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        if (track.muted || (hasSolo && !track.solo)) continue;
        track.processedBuffer.clear(0, sampleCount);
        const float* inputChannels[2] = {
            track.mixBuffer.getWritePointer(0), track.mixBuffer.getWritePointer(1)};
        float* processedChannels[2] = {
            track.processedBuffer.getWritePointer(0), track.processedBuffer.getWritePointer(1)};
        if (track.rack != nullptr)
            track.rack->process(inputChannels, 2, processedChannels, 2, sampleCount);
        else {
            juce::FloatVectorOperations::copy(processedChannels[0], inputChannels[0], sampleCount);
            juce::FloatVectorOperations::copy(processedChannels[1], inputChannels[1], sampleCount);
        }
        const auto panAngle = (track.pan + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const auto leftGain = track.gain * std::cos(panAngle);
        const auto rightGain = track.gain * std::sin(panAngle);
        const auto delay = track.compensationDelaySamples;
        const auto delaySize = track.delayBuffer.getNumSamples();
        for (int sample = 0; sample < sampleCount; ++sample) {
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
            if (channelCount > 0 && outputChannels[0] != nullptr)
                outputChannels[0][destinationStart + sample] += left * leftGain;
            if (channelCount > 1 && outputChannels[1] != nullptr)
                outputChannels[1][destinationStart + sample] += right * rightGain;
        }
    }
}

void TimelineEngine::resetTrackState(PreparedTimeline& prepared) noexcept {
    for (auto& trackPtr : prepared.tracks) {
        auto& track = *trackPtr;
        for (auto& clip : track.clips) clip->expectedSourceFrame = -1;
        track.mixBuffer.clear();
        track.processedBuffer.clear();
        track.delayBuffer.clear();
        track.delayWritePosition = 0;
    }
}

void TimelineEngine::mix(
    float* const* outputChannels,
    const int channelCount,
    const int sampleCount) noexcept {
    audioClockSample.fetch_add(static_cast<std::uint64_t>(sampleCount), std::memory_order_relaxed);
    if (state.load(std::memory_order_acquire) != State::playing) return;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return;
    auto position = timelineSample.load(std::memory_order_relaxed);
    auto consumed = 0;
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
        processTracks(*timeline, outputChannels, channelCount, consumed, chunk);
        position += chunk;
        consumed += chunk;
        if (timeline->loopEnabled && position >= timeline->loopEndSample) {
            position = timeline->loopStartSample;
            resetTrackState(*timeline);
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
    object->setProperty("clockGeneration", static_cast<juce::int64>(
        clockGeneration.load(std::memory_order_acquire)));
    object->setProperty("discontinuity", static_cast<juce::int64>(
        discontinuity.load(std::memory_order_acquire)));
    object->setProperty("revision", 0);
    object->setProperty("sampleRate", 0.0);
    object->setProperty("timelineTick", 0);
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (lock.isLocked() && timeline != nullptr) {
        object->setProperty("revision", static_cast<juce::int64>(timeline->revision));
        object->setProperty("sampleRate", timeline->outputSampleRate);
        const auto tick = timeline->outputSampleRate > 0.0
            ? static_cast<juce::int64>(std::llround(
                static_cast<double>(timelineSample.load(std::memory_order_acquire)) *
                timeline->bpm * static_cast<double>(timeline->ppq) /
                (timeline->outputSampleRate * 60.0)))
            : 0;
        object->setProperty("timelineTick", tick);
    }
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
    juce::String error;
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
        auto* rack = new juce::DynamicObject();
        rack->setProperty("devices", juce::Array<juce::var> {});
        audioTrack->setProperty("rack", juce::var(rack));
        audioTrack->setProperty("audioClips", clips);
        audioTrack->setProperty("midiClips", juce::Array<juce::var> {});
        juce::Array<juce::var> tracks;
        tracks.add(juce::var(audioTrack));
        auto* snapshotObject = new juce::DynamicObject();
        snapshotObject->setProperty("revision", 7);
        snapshotObject->setProperty("timebase", juce::var(timebase));
        snapshotObject->setProperty("loopRange", juce::var(loopRange));
        snapshotObject->setProperty("tracks", tracks);
        loaded = engine.loadSnapshot(
            juce::var(snapshotObject), formats, 48000.0, 512, error);
        if (loaded) {
            engine.play();
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            std::array<float, 512> left {};
            std::array<float, 512> right {};
            std::array<float*, 2> channels { left.data(), right.data() };
            for (int block = 0; block < 8; ++block)
                engine.mix(channels.data(), 2, static_cast<int>(left.size()));
            const auto peak = std::max(
                *std::max_element(left.begin(), left.end()),
                *std::max_element(right.begin(), right.end()));
            mixed = peak > 0.1f;
            engine.seekToTick(960);
            const auto seekStatus = engine.status();
            seeked = static_cast<juce::int64>(seekStatus.getProperty("timelineSample", -1)) == 24000;

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
            loopSnapshot->setProperty("revision", 8);
            loopSnapshot->setProperty("timebase", juce::var(loopTimebase));
            loopSnapshot->setProperty("loopRange", juce::var(enabledLoop));
            auto* loopTrack = new juce::DynamicObject();
            loopTrack->setProperty("id", "track:loop");
            loopTrack->setProperty("gainDb", 0.0);
            loopTrack->setProperty("pan", 0.0);
            loopTrack->setProperty("muted", false);
            loopTrack->setProperty("solo", false);
            auto* loopRack = new juce::DynamicObject();
            loopRack->setProperty("devices", juce::Array<juce::var> {});
            loopTrack->setProperty("rack", juce::var(loopRack));
            loopTrack->setProperty("audioClips", juce::Array<juce::var> {});
            loopTrack->setProperty("midiClips", juce::Array<juce::var> {});
            juce::Array<juce::var> loopTracks;
            loopTracks.add(juce::var(loopTrack));
            loopSnapshot->setProperty("tracks", loopTracks);
            if (engine.loadSnapshot(
                    juce::var(loopSnapshot), formats, 48000.0, 512, error)) {
                engine.seekToTick(0);
                std::array<float, 24000> silent {};
                std::array<float*, 1> silentChannels { silent.data() };
                engine.mix(silentChannels.data(), 1, static_cast<int>(silent.size()));
                looped = static_cast<juce::int64>(
                    engine.status().getProperty("timelineSample", -1)) == 0;
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
    addCheck("overlapping sources mix through read-ahead and sample-rate correction", mixed);
    addCheck("tick seek resolves against the engine sample clock", seeked);
    addCheck("loop wrap returns to the exact loop start", looped);
    result->setProperty("checks", checks);
    result->setProperty("message", error);
    result->setProperty("passed", sourcesWritten && loaded && mixed && seeked && looped);
    mono.deleteFile();
    stereo.deleteFile();
    return juce::var(result);
}

} // namespace riffra
