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

    const auto clips = snapshot.getProperty("audioClips", {});
    if (!clips.isArray()) {
        error = "Timeline snapshot audioClips must be an array.";
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
        prepared->clips.push_back(std::move(clip));
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
    for (auto& clip : timeline->clips) clip->expectedSourceFrame = -1;
    discontinuity.fetch_add(1, std::memory_order_relaxed);
    sequence.fetch_add(1, std::memory_order_relaxed);
}

void TimelineEngine::mixRange(
    PreparedTimeline& prepared,
    const std::int64_t rangeStart,
    float* const* outputChannels,
    const int channelCount,
    const int destinationStart,
    const int sampleCount) noexcept {
    const auto rangeEnd = rangeStart + sampleCount;
    for (auto& clipPtr : prepared.clips) {
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
                prepared.outputSampleRate));
            if (clip.loop) sourceOffset %= sourceRange;
            auto sourceFrame = clip.sourceStartFrame + sourceOffset;
            if (sourceFrame >= clip.sourceEndFrame) break;
            const auto sourceRemaining = clip.sourceEndFrame - sourceFrame;
            const auto outputUntilSourceEnd = static_cast<int>(std::ceil(
                static_cast<double>(sourceRemaining) * prepared.outputSampleRate /
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
                for (int channel = 0; channel < channelCount; ++channel) {
                    if (outputChannels[channel] == nullptr) continue;
                    const auto sourceChannel = std::min(channel, clip.scratch.getNumChannels() - 1);
                    const auto panAngle = (clip.pan + 1.0f) *
                        juce::MathConstants<float>::pi * 0.25f;
                    const auto panGain = channel == 0
                        ? std::cos(panAngle)
                        : std::sin(panAngle);
                    outputChannels[channel][outputOffset + sample] +=
                        clip.scratch.getSample(sourceChannel, sample) * clip.gain * envelope * panGain;
                }
            }
            clip.expectedSourceFrame = sourceFrame + static_cast<std::int64_t>(std::floor(
                static_cast<double>(chunk) * clip.sourceSampleRate /
                prepared.outputSampleRate));
            remaining -= chunk;
            outputOffset += chunk;
            localSample += chunk;
            if (!clip.loop && sourceFrame + sourceRemaining >= clip.sourceEndFrame && remaining > 0)
                break;
            if (clip.loop && remaining > 0) clip.expectedSourceFrame = -1;
        }
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
        if (timeline->loopEnabled && position < timeline->loopEndSample) {
            chunk = std::min<int>(
                chunk,
                static_cast<int>(timeline->loopEndSample - position));
        }
        mixRange(*timeline, position, outputChannels, channelCount, consumed, chunk);
        position += chunk;
        consumed += chunk;
        if (timeline->loopEnabled && position >= timeline->loopEndSample) {
            position = timeline->loopStartSample;
            for (auto& clip : timeline->clips) clip->expectedSourceFrame = -1;
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
        auto* snapshotObject = new juce::DynamicObject();
        snapshotObject->setProperty("revision", 7);
        snapshotObject->setProperty("timebase", juce::var(timebase));
        snapshotObject->setProperty("loopRange", juce::var(loopRange));
        snapshotObject->setProperty("audioClips", clips);
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
            loopSnapshot->setProperty("audioClips", juce::Array<juce::var> {});
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
