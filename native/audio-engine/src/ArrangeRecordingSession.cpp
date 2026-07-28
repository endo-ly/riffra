#include "ArrangeRecordingSession.h"

#include <algorithm>
#include <map>
#include <optional>
#include <utility>

namespace riffra {

std::unique_ptr<ArrangeRecordingSession> ArrangeRecordingSession::create(
    const juce::File& directory,
    const juce::var& configuration,
    juce::String& error) {
    const auto rate = static_cast<double>(configuration.getProperty("sampleRate", 0.0));
    if (!configuration.isObject() || rate <= 0.0) {
        error = "Arrange recording requires a valid runtime configuration.";
        return {};
    }
    auto result = std::unique_ptr<ArrangeRecordingSession>(
        new ArrangeRecordingSession(directory, rate));
    if (!result->initialise(configuration, error))
        return {};
    return result;
}

ArrangeRecordingSession::ArrangeRecordingSession(
    juce::File target,
    const double rate)
    : directory(std::move(target)),
      manifest(directory.getChildFile("manifest.json")),
      sampleRate(rate) {}

bool ArrangeRecordingSession::initialise(
    const juce::var& configuration,
    juce::String& error) {
    if (!directory.createDirectory()
        || !directory.getChildFile("tracks").createDirectory()) {
        error = "Arrange recording folders could not be created.";
        return false;
    }
    timelineStartTick = static_cast<std::uint64_t>(static_cast<juce::int64>(
        configuration.getProperty("timelineStartTick", 0)));
    loopEnabled = static_cast<bool>(configuration.getProperty("loopEnabled", false));
    loopStartSample = static_cast<juce::int64>(
        configuration.getProperty("loopStartSample", 0));
    loopEndSample = static_cast<juce::int64>(
        configuration.getProperty("loopEndSample", 0));
    punchEnabled = static_cast<bool>(configuration.getProperty("punchEnabled", false));
    punchStartSample = static_cast<juce::int64>(
        configuration.getProperty("punchStartSample", 0));
    punchEndSample = static_cast<juce::int64>(
        configuration.getProperty("punchEndSample", 0));
    const auto values = configuration.getProperty("tracks", {});
    if (!values.isArray() || values.size() == 0) {
        error = "Arrange recording requires at least one armed Track.";
        return false;
    }
    tracks.reserve(static_cast<std::size_t>(values.size()));
    for (int index = 0; index < values.size(); ++index) {
        const auto value = values[index];
        TrackWriter track;
        track.trackId = value.getProperty("trackId", {}).toString();
        track.trackKey = juce::String(index).paddedLeft('0', 4);
        track.kind = value.getProperty("kind", {}).toString();
        track.audioInputChannel = static_cast<int>(
            value.getProperty("audioInputChannel", -1));
        track.midiDeviceId = value.getProperty("midiDeviceId", {}).toString();
        track.midiChannel = static_cast<int>(value.getProperty("midiChannel", 0));
        track.pluginLatencySamples = static_cast<int>(
            value.getProperty("pluginLatencySamples", 0));
        track.pluginTailSamples = static_cast<int>(
            value.getProperty("pluginTailSamples", 0));
        track.captureSegments.reserve(TrackWriter::kMaximumTrackCaptureSegments);
        if (track.trackId.isEmpty()) {
            error = "An armed Track has no stable ID.";
            return false;
        }
        if (track.kind == "audio") {
            const auto child = directory.getChildFile("tracks").getChildFile(track.trackKey);
            track.audio = RecordingSession::create(child, sampleRate, 1, 2, error);
            if (track.audio == nullptr)
                return false;
        } else if (!directory.getChildFile("tracks")
                        .getChildFile(track.trackKey).createDirectory()) {
            error = "MIDI Track recording folder could not be created.";
            return false;
        }
        tracks.push_back(std::move(track));
    }
    return writeManifest("recording", error);
}

bool ArrangeRecordingSession::beginAudioTrackCapture(
    const juce::String& trackId,
    const std::uint64_t audioClockStartSample,
    const std::uint64_t timelineStartSample) noexcept {
    if (finished.load(std::memory_order_acquire))
        return false;
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr || found->captureActive
        || found->tailActive
        || found->captureSegmentCount >= found->captureSegments.capacity())
        return false;
    found->captureSegments.emplace_back();
    auto& segment = found->captureSegments[found->captureSegmentCount++];
    segment.audioClockStartSample = audioClockStartSample;
    segment.timelineStartSample = timelineStartSample;
    segment.rawFileStartSample = found->audio->getRawSamplesWritten();
    segment.rawFileEndSample = segment.rawFileStartSample;
    if (loopEnabled) {
        // Loop recording: processed will be generated offline with same layout as raw
        segment.processedFileStartSample = segment.rawFileStartSample;
        segment.processedFileEndSample = segment.rawFileStartSample;
        segment.processedTailEndSample = segment.rawFileStartSample;
    } else {
        segment.processedFileStartSample = found->audio->getProcessedSamplesWritten();
        segment.processedFileEndSample = segment.processedFileStartSample;
        segment.processedTailEndSample = segment.processedFileStartSample;
    }
    found->captureActive = true;
    return true;
}

void ArrangeRecordingSession::writeAudioTrack(
    const juce::String& trackId,
    const float* raw,
    const int rawSampleCount,
    const float* const* processed,
    const int processedSampleCount) noexcept {
    if (finished.load(std::memory_order_acquire)
        || (rawSampleCount > 0 && raw == nullptr)
        || (processedSampleCount > 0 && processed == nullptr))
        return;
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr)
        return;
    if (rawSampleCount > 0) {
        const std::array<const float*, 1> rawChannels { raw };
        (void) found->audio->writeRaw(rawChannels.data(), rawSampleCount);
    }
    if (processedSampleCount > 0)
        (void) found->audio->writeProcessed(processed, processedSampleCount);
    if (found->tailActive && found->tailSegmentIndex < found->captureSegmentCount)
        found->captureSegments[found->tailSegmentIndex].processedTailEndSample =
            found->audio->getProcessedSamplesWritten();
}

bool ArrangeRecordingSession::endAudioTrackCapture(
    const juce::String& trackId,
    const std::uint64_t audioClockEndSample,
    const std::uint64_t timelineEndSample) noexcept {
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr || !found->captureActive
        || found->captureSegmentCount == 0)
        return false;
    auto& segment = found->captureSegments[found->captureSegmentCount - 1];
    segment.audioClockEndSample = audioClockEndSample;
    segment.timelineEndSample = timelineEndSample;
    segment.rawFileEndSample = found->audio->getRawSamplesWritten();
    if (loopEnabled) {
        // Loop recording: processed mirrors raw layout (generated offline after stop)
        segment.processedFileStartSample = segment.rawFileStartSample;
        segment.processedFileEndSample = segment.rawFileEndSample;
        segment.processedTailEndSample = segment.rawFileEndSample;
        found->captureActive = false;
        found->tailActive = false;
    } else {
        segment.processedFileEndSample = segment.processedFileStartSample
            + (segment.rawFileEndSample - segment.rawFileStartSample);
        segment.processedTailEndSample = found->audio->getProcessedSamplesWritten();
        found->captureActive = false;
        found->tailActive = true;
        found->tailSegmentIndex = found->captureSegmentCount - 1;
    }
    return true;
}

bool ArrangeRecordingSession::completeAudioTrackTail(
    const juce::String& trackId) noexcept {
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr || !found->tailActive
        || found->tailSegmentIndex >= found->captureSegmentCount)
        return false;
    auto& segment = found->captureSegments[found->tailSegmentIndex];
    segment.processedTailEndSample = found->audio->getProcessedSamplesWritten();
    found->tailActive = false;
    return true;
}

void ArrangeRecordingSession::markLoopBoundary(
    const std::uint64_t audioSample) noexcept {
    const auto index = loopBoundaryCount.fetch_add(1, std::memory_order_relaxed);
    if (index < loopBoundaries.size())
        loopBoundaries[index].store(audioSample, std::memory_order_release);
    else
        loopBoundaryCount.store(loopBoundaries.size(), std::memory_order_release);
}

void ArrangeRecordingSession::writeMidiTrack(
    const juce::String& trackId,
    const juce::String& sourceDeviceId,
    const juce::MidiMessage& message,
    const std::uint64_t audioSample) noexcept {
    if (finished.load(std::memory_order_acquire))
        return;
    const juce::ScopedLock lock(midiLock);
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId && track.kind == "instrument";
    });
    if (found == tracks.end() || found->midiEvents.size() >= 200'000)
        return;
    const auto* bytes = message.getRawData();
    found->midiEvents.push_back(TrackWriter::MidiEvent {
        audioSample,
        sourceDeviceId,
        message.getRawDataSize() > 0 ? bytes[0] & 0xf0 : 0,
        message.getChannel(),
        message.getRawDataSize() > 1 ? bytes[1] : 0,
        message.getRawDataSize() > 2 ? bytes[2] : 0,
    });
}

void ArrangeRecordingSession::setCaptureRange(
    const std::uint64_t startAudioSample,
    const std::uint64_t endAudioSample,
    const std::uint64_t startTimelineSample,
    const std::uint64_t endTimelineSample) noexcept {
    if (endAudioSample <= startAudioSample || endTimelineSample <= startTimelineSample)
        return;
    const auto sampleCount = std::min(
        endAudioSample - startAudioSample,
        endTimelineSample - startTimelineSample);
    auto count = captureSegmentCount.load(std::memory_order_relaxed);
    auto coalesced = false;
    if (count > 0) {
        auto& previous = captureSegments[count - 1];
        if (previous.audioClockEndSample == startAudioSample
            && previous.timelineEndSample == startTimelineSample
            && previous.fileEndSample == capturedFileSamples) {
            previous.audioClockEndSample = startAudioSample + sampleCount;
            previous.timelineEndSample = startTimelineSample + sampleCount;
            previous.fileEndSample += sampleCount;
            capturedFileSamples += sampleCount;
            coalesced = true;
        }
    }
    if (!coalesced && count < captureSegments.size()) {
        captureSegments[count] = CaptureSegment {
            startAudioSample,
            startAudioSample + sampleCount,
            startTimelineSample,
            startTimelineSample + sampleCount,
            capturedFileSamples,
            capturedFileSamples + sampleCount,
        };
        capturedFileSamples += sampleCount;
        captureSegmentCount.store(count + 1, std::memory_order_release);
    }
    auto currentStart = recordStartAudioSample.load(std::memory_order_acquire);
    while (startAudioSample < currentStart
           && !recordStartAudioSample.compare_exchange_weak(
               currentStart, startAudioSample, std::memory_order_acq_rel))
        {}
    if (startAudioSample == recordStartAudioSample.load(std::memory_order_acquire))
        recordStartTimelineSample.store(startTimelineSample, std::memory_order_release);
    auto currentEnd = recordEndAudioSample.load(std::memory_order_acquire);
    while (endAudioSample > currentEnd
           && !recordEndAudioSample.compare_exchange_weak(
               currentEnd, endAudioSample, std::memory_order_acq_rel))
        {}
    if (endAudioSample == recordEndAudioSample.load(std::memory_order_acquire))
        recordEndTimelineSample.store(endTimelineSample, std::memory_order_release);
}

juce::File ArrangeRecordingSession::prepareRawForReading(
    const juce::String& trackId) noexcept {
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr)
        return {};
    return found->audio->flushRaw();
}

std::vector<std::pair<std::uint64_t, std::uint64_t>>
ArrangeRecordingSession::getRawSegmentRanges(
    const juce::String& trackId) noexcept {
    std::vector<std::pair<std::uint64_t, std::uint64_t>> result;
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end())
        return result;
    result.reserve(found->captureSegmentCount);
    for (std::size_t i = 0; i < found->captureSegmentCount; ++i) {
        const auto& segment = found->captureSegments[i];
        if (segment.rawFileEndSample > segment.rawFileStartSample)
            result.emplace_back(segment.rawFileStartSample, segment.rawFileEndSample);
    }
    return result;
}

bool ArrangeRecordingSession::finish(juce::String& error) {
    if (finished.exchange(true, std::memory_order_acq_rel))
        return true;
    auto completed = true;
    for (auto& track : tracks) {
        if (track.audio != nullptr) {
            juce::String trackError;
            if (!track.audio->finish(trackError)) {
                completed = false;
                error << track.trackId << ": " << trackError << " ";
            }
        }
        if (track.kind == "instrument") {
            juce::Array<juce::var> events;
            {
                const juce::ScopedLock lock(midiLock);
                const auto segmentCount = std::min(
                    captureSegmentCount.load(std::memory_order_acquire),
                    captureSegments.size());
                const auto appendEvent = [&events](
                                             const TrackWriter::MidiEvent& event,
                                             const std::uint64_t sampleOffset) {
                    auto* value = new juce::DynamicObject();
                    value->setProperty("sampleOffset", static_cast<juce::int64>(sampleOffset));
                    value->setProperty("sourceDeviceId", event.sourceDeviceId);
                    value->setProperty("status", event.status);
                    value->setProperty("channel", event.channel);
                    value->setProperty("data1", event.data1);
                    value->setProperty("data2", event.data2);
                    events.add(juce::var(value));
                };
                // Open notes are scoped to one capture segment. This prevents a
                // Punch gap or loop restart from borrowing a Note Off from a
                // later pass, and closes a captured note exactly at the pass end.
                for (std::size_t segmentIndex = 0; segmentIndex < segmentCount; ++segmentIndex) {
                    const auto& segment = captureSegments[segmentIndex];
                    std::map<std::pair<int, int>, TrackWriter::MidiEvent> openNotes;
                    for (const auto& event : track.midiEvents) {
                        if (event.audioSample < segment.audioClockStartSample
                            || event.audioSample >= segment.audioClockEndSample)
                            continue;
                        const auto kind = event.status & 0xf0;
                        const auto key = std::make_pair(event.channel, event.data1);
                        const auto isNoteOff = kind == 0x80 || (kind == 0x90 && event.data2 == 0);
                        if (isNoteOff) {
                            // A Note On before the punch is not captured, so its
                            // matching Note Off must not create a dangling event.
                            if (openNotes.erase(key) == 0)
                                continue;
                        } else if (kind == 0x90) {
                            openNotes[key] = event;
                        }
                        appendEvent(
                            event,
                            segment.fileStartSample
                                + event.audioSample - segment.audioClockStartSample);
                    }
                    for (const auto& [key, noteOn] : openNotes) {
                        auto syntheticOff = noteOn;
                        syntheticOff.status = 0x80;
                        syntheticOff.channel = key.first;
                        syntheticOff.data1 = key.second;
                        syntheticOff.data2 = 0;
                        appendEvent(syntheticOff, segment.fileEndSample);
                    }
                }
            }
            auto* root = new juce::DynamicObject();
            root->setProperty("version", 2);
            root->setProperty("sampleRate", sampleRate);
            root->setProperty("events", events);
            const auto midiFile = directory.getChildFile("tracks")
                .getChildFile(track.trackKey).getChildFile("midi.json");
            if (!midiFile.replaceWithText(juce::JSON::toString(juce::var(root), true))) {
                completed = false;
                error << track.trackId << ": MIDI recording could not be finalized. ";
            }
        }
    }
    juce::String manifestError;
    if (!writeManifest(completed ? "completed" : "recoverable", manifestError)) {
        error << manifestError;
        return false;
    }
    return completed;
}

bool ArrangeRecordingSession::cancel(juce::String& error) {
    if (finished.exchange(true, std::memory_order_acq_rel))
        return true;
    for (auto& track : tracks) {
        if (track.audio != nullptr) {
            juce::String ignored;
            (void) track.audio->finish(ignored);
            track.audio.reset();
        }
    }
    if (directory.exists() && !directory.deleteRecursively()) {
        error = "Cancelled recording files could not be removed.";
        return false;
    }
    return true;
}

juce::var ArrangeRecordingSession::status() const {
    auto* result = new juce::DynamicObject();
    result->setProperty("active", !finished.load(std::memory_order_acquire));
    result->setProperty("directory", directory.getFullPathName());
    result->setProperty("sampleRate", sampleRate);
    std::uint64_t written = 0;
    std::uint64_t dropped = 0;
    std::uint64_t rawMissing = 0;
    std::uint64_t processedMissing = 0;
    for (const auto& track : tracks) {
        if (track.audio != nullptr) {
            written = std::max(written, track.audio->getSamplesWritten());
            dropped += track.audio->getDroppedBlocks();
            rawMissing += track.audio->getRawMissingSamples();
            processedMissing += track.audio->getProcessedMissingSamples();
        }
    }
    result->setProperty("samplesWritten", static_cast<juce::int64>(written));
    result->setProperty("droppedBlocks", static_cast<juce::int64>(dropped));
    result->setProperty("rawMissingSamples", static_cast<juce::int64>(rawMissing));
    result->setProperty("processedMissingSamples", static_cast<juce::int64>(processedMissing));
    result->setProperty("recoveryStatus", dropped == 0 ? "clean" : "partial");
    return juce::var(result);
}

bool ArrangeRecordingSession::writeManifest(
    const juce::String& state,
    juce::String& error) const {
    auto rootValue = manifest.existsAsFile()
        ? juce::JSON::parse(manifest.loadFileAsString())
        : juce::var {};
    if (!rootValue.isObject())
        rootValue = juce::var(new juce::DynamicObject());
    auto* root = rootValue.getDynamicObject();
    root->setProperty("state", state);
    root->setProperty("captureId", directory.getFileName());
    root->setProperty("sampleRate", sampleRate);
    const auto captureStart = recordStartAudioSample.load(std::memory_order_acquire);
    root->setProperty("recordStartAudioSample", static_cast<juce::int64>(
        captureStart == std::numeric_limits<std::uint64_t>::max() ? 0 : captureStart));
    root->setProperty("recordEndAudioSample", static_cast<juce::int64>(
        recordEndAudioSample.load(std::memory_order_acquire)));
    const auto timelineCaptureStart =
        recordStartTimelineSample.load(std::memory_order_acquire);
    root->setProperty("recordStartTimelineSample", static_cast<juce::int64>(
        timelineCaptureStart == std::numeric_limits<std::uint64_t>::max()
            ? 0
            : timelineCaptureStart));
    root->setProperty("recordEndTimelineSample", static_cast<juce::int64>(
        recordEndTimelineSample.load(std::memory_order_acquire)));
    root->setProperty("timelineStartTick", static_cast<juce::int64>(timelineStartTick));
    std::uint64_t samplesWritten = 0;
    std::uint64_t droppedBlocks = 0;
    std::uint64_t missingSamples = 0;
    std::uint64_t rawAttemptedSamples = 0;
    std::uint64_t processedAttemptedSamples = 0;
    std::uint64_t rawDroppedBlocks = 0;
    std::uint64_t processedDroppedBlocks = 0;
    std::uint64_t rawMissingSamples = 0;
    std::uint64_t processedMissingSamples = 0;
    std::optional<std::uint64_t> rawFirstMissingSample;
    std::optional<std::uint64_t> rawLastMissingSample;
    std::optional<std::uint64_t> processedFirstMissingSample;
    std::optional<std::uint64_t> processedLastMissingSample;
    for (const auto& track : tracks) {
        if (track.audio != nullptr) {
            samplesWritten = std::max(samplesWritten, track.audio->getSamplesWritten());
            droppedBlocks += track.audio->getDroppedBlocks();
            missingSamples += track.audio->getMissingSamples();
            rawAttemptedSamples += track.audio->getRawAttemptedSamples();
            processedAttemptedSamples += track.audio->getProcessedAttemptedSamples();
            rawDroppedBlocks += track.audio->getRawDroppedBlocks();
            processedDroppedBlocks += track.audio->getProcessedDroppedBlocks();
            rawMissingSamples += track.audio->getRawMissingSamples();
            processedMissingSamples += track.audio->getProcessedMissingSamples();
            const auto rawFirst = track.audio->getRawFirstMissingSample();
            const auto processedFirst = track.audio->getProcessedFirstMissingSample();
            if (track.audio->getRawMissingSamples() > 0)
                rawFirstMissingSample = rawFirstMissingSample.has_value()
                    ? std::min(*rawFirstMissingSample, rawFirst)
                    : rawFirst;
            if (track.audio->getProcessedMissingSamples() > 0)
                processedFirstMissingSample = processedFirstMissingSample.has_value()
                    ? std::min(*processedFirstMissingSample, processedFirst)
                    : processedFirst;
            const auto rawLast = track.audio->getRawLastMissingSample();
            const auto processedLast = track.audio->getProcessedLastMissingSample();
            if (track.audio->getRawMissingSamples() > 0)
                rawLastMissingSample = std::max(rawLastMissingSample.value_or(0), rawLast);
            if (track.audio->getProcessedMissingSamples() > 0)
                processedLastMissingSample =
                    std::max(processedLastMissingSample.value_or(0), processedLast);
        }
    }
    root->setProperty("samplesWritten", static_cast<juce::int64>(samplesWritten));
    root->setProperty("droppedBlocks", static_cast<juce::int64>(droppedBlocks));
    root->setProperty("missingSamples", static_cast<juce::int64>(missingSamples));
    root->setProperty("rawAttemptedSamples", static_cast<juce::int64>(rawAttemptedSamples));
    root->setProperty(
        "processedAttemptedSamples", static_cast<juce::int64>(processedAttemptedSamples));
    root->setProperty("rawDroppedBlocks", static_cast<juce::int64>(rawDroppedBlocks));
    root->setProperty(
        "processedDroppedBlocks", static_cast<juce::int64>(processedDroppedBlocks));
    root->setProperty("rawMissingSamples", static_cast<juce::int64>(rawMissingSamples));
    root->setProperty(
        "processedMissingSamples", static_cast<juce::int64>(processedMissingSamples));
    root->setProperty(
        "rawDropoutStartSample",
        static_cast<juce::int64>(rawFirstMissingSample.value_or(0)));
    root->setProperty(
        "rawDropoutEndSample",
        static_cast<juce::int64>(rawLastMissingSample.value_or(0)));
    root->setProperty(
        "processedDropoutStartSample",
        static_cast<juce::int64>(processedFirstMissingSample.value_or(0)));
    root->setProperty(
        "processedDropoutEndSample",
        static_cast<juce::int64>(processedLastMissingSample.value_or(0)));
    root->setProperty("recoveryStatus", droppedBlocks == 0 ? "clean" : "partial");
    juce::Array<juce::var> segments;
    const auto segmentCount = std::min(
        captureSegmentCount.load(std::memory_order_acquire),
        captureSegments.size());
    for (std::size_t index = 0; index < segmentCount; ++index) {
        const auto& segment = captureSegments[index];
        auto* value = new juce::DynamicObject();
        value->setProperty("audioClockStartSample", static_cast<juce::int64>(
            segment.audioClockStartSample));
        value->setProperty("audioClockEndSample", static_cast<juce::int64>(
            segment.audioClockEndSample));
        value->setProperty("timelineStartSample", static_cast<juce::int64>(
            segment.timelineStartSample));
        value->setProperty("timelineEndSample", static_cast<juce::int64>(
            segment.timelineEndSample));
        value->setProperty("fileStartSample", static_cast<juce::int64>(
            segment.fileStartSample));
        value->setProperty("fileEndSample", static_cast<juce::int64>(
            segment.fileEndSample));
        segments.add(juce::var(value));
    }
    root->setProperty("captureSegments", segments);
    juce::Array<juce::var> boundaries;
    const auto count = std::min(loopBoundaryCount.load(std::memory_order_acquire),
                                loopBoundaries.size());
    for (std::size_t index = 0; index < count; ++index)
        boundaries.add(static_cast<juce::int64>(
            loopBoundaries[index].load(std::memory_order_acquire)));
    root->setProperty("loopBoundariesSample", boundaries);
    auto* loop = new juce::DynamicObject();
    loop->setProperty("enabled", loopEnabled);
    loop->setProperty("startSample", static_cast<juce::int64>(loopStartSample));
    loop->setProperty("endSample", static_cast<juce::int64>(loopEndSample));
    root->setProperty("loopRange", juce::var(loop));
    auto* punch = new juce::DynamicObject();
    punch->setProperty("enabled", punchEnabled);
    punch->setProperty("startSample", static_cast<juce::int64>(punchStartSample));
    punch->setProperty("endSample", static_cast<juce::int64>(punchEndSample));
    root->setProperty("punchRange", juce::var(punch));
    juce::Array<juce::var> trackValues;
    for (const auto& track : tracks) {
        auto* value = new juce::DynamicObject();
        value->setProperty("trackId", track.trackId);
        value->setProperty("trackKey", track.trackKey);
        value->setProperty("kind", track.kind);
        auto* audioInput = new juce::DynamicObject();
        audioInput->setProperty("channelIndex", track.audioInputChannel);
        value->setProperty("audioInput", juce::var(audioInput));
        auto* midiInput = new juce::DynamicObject();
        midiInput->setProperty("deviceId", track.midiDeviceId);
        midiInput->setProperty("channel", track.midiChannel);
        value->setProperty("midiInput", juce::var(midiInput));
        value->setProperty("pluginLatencySamples", track.pluginLatencySamples);
        value->setProperty("pluginTailSamples", track.pluginTailSamples);
        if (track.kind == "audio") {
            if (track.audio != nullptr) {
                value->setProperty(
                    "rawAttemptedSamples",
                    static_cast<juce::int64>(track.audio->getRawAttemptedSamples()));
                value->setProperty(
                    "processedAttemptedSamples",
                    static_cast<juce::int64>(track.audio->getProcessedAttemptedSamples()));
                value->setProperty(
                    "rawMissingSamples",
                    static_cast<juce::int64>(track.audio->getRawMissingSamples()));
                value->setProperty(
                    "processedMissingSamples",
                    static_cast<juce::int64>(track.audio->getProcessedMissingSamples()));
                value->setProperty(
                    "rawDropoutStartSample",
                    static_cast<juce::int64>(track.audio->getRawFirstMissingSample()));
                value->setProperty(
                    "rawDropoutEndSample",
                    static_cast<juce::int64>(track.audio->getRawLastMissingSample()));
                value->setProperty(
                    "processedDropoutStartSample",
                    static_cast<juce::int64>(track.audio->getProcessedFirstMissingSample()));
                value->setProperty(
                    "processedDropoutEndSample",
                    static_cast<juce::int64>(track.audio->getProcessedLastMissingSample()));
            }
            value->setProperty("rawFile", "tracks/" + track.trackKey + "/raw.wav");
            value->setProperty(
                "processedFile", "tracks/" + track.trackKey + "/processed.wav");
            juce::Array<juce::var> variantSegments;
            const auto trackSegmentCount = std::min(
                track.captureSegmentCount, track.captureSegments.size());
            for (std::size_t index = 0; index < trackSegmentCount; ++index) {
                const auto& segment = track.captureSegments[index];
                auto* mapped = new juce::DynamicObject();
                mapped->setProperty("audioClockStartSample", static_cast<juce::int64>(
                    segment.audioClockStartSample));
                mapped->setProperty("audioClockEndSample", static_cast<juce::int64>(
                    segment.audioClockEndSample));
                mapped->setProperty("timelineStartSample", static_cast<juce::int64>(
                    segment.timelineStartSample));
                mapped->setProperty("timelineEndSample", static_cast<juce::int64>(
                    segment.timelineEndSample));
                mapped->setProperty("rawFileStartSample", static_cast<juce::int64>(
                    segment.rawFileStartSample));
                mapped->setProperty("rawFileEndSample", static_cast<juce::int64>(
                    segment.rawFileEndSample));
                mapped->setProperty("processedFileStartSample", static_cast<juce::int64>(
                    segment.processedFileStartSample));
                mapped->setProperty("processedFileEndSample", static_cast<juce::int64>(
                    segment.processedFileEndSample));
                mapped->setProperty("processedTailEndSample", static_cast<juce::int64>(
                    segment.processedTailEndSample));
                variantSegments.add(juce::var(mapped));
            }
            value->setProperty("captureSegments", variantSegments);
        } else {
            value->setProperty("midiFile", "tracks/" + track.trackKey + "/midi.json");
        }
        trackValues.add(juce::var(value));
    }
    root->setProperty("tracks", trackValues);
    if (!manifest.replaceWithText(juce::JSON::toString(rootValue, true))) {
        error = "Arrange recording manifest could not be written.";
        return false;
    }
    return true;
}

juce::var runArrangeRecordingSelfTest(const juce::File& directory) {
    directory.createDirectory();
    auto* configuration = new juce::DynamicObject();
    configuration->setProperty("sampleRate", 48000.0);
    configuration->setProperty("timelineStartTick", 960);
    configuration->setProperty("loopEnabled", true);
    configuration->setProperty("loopStartSample", 24000);
    configuration->setProperty("loopEndSample", 48000);
    configuration->setProperty("punchEnabled", false);
    juce::Array<juce::var> tracks;
    const auto addTrack = [&tracks](
                              const juce::String& id,
                              const juce::String& kind,
                              const int input) {
        auto* track = new juce::DynamicObject();
        track->setProperty("trackId", id);
        track->setProperty("kind", kind);
        track->setProperty("audioInputChannel", input);
        track->setProperty("pluginLatencySamples", input + 8);
        track->setProperty("pluginTailSamples", input + 16);
        tracks.add(juce::var(track));
    };
    addTrack("track:guitar", "audio", 0);
    addTrack("track:vocal", "audio", 1);
    addTrack("track:keys", "instrument", -1);
    configuration->setProperty("tracks", tracks);
    const auto configurationValue = juce::var(configuration);
    juce::String error;
    auto session = ArrangeRecordingSession::create(
        directory, configurationValue, error);
    bool written = session != nullptr;
    if (session != nullptr) {
        const auto manifestFile = directory.getChildFile("manifest.json");
        auto manifestValue = juce::JSON::parse(manifestFile.loadFileAsString());
        if (auto* manifestObject = manifestValue.getDynamicObject()) {
            auto* capture = new juce::DynamicObject();
            capture->setProperty("captureId", "capture:self-test");
            manifestObject->setProperty("capture", juce::var(capture));
            written = manifestFile.replaceWithText(
                juce::JSON::toString(manifestValue, true));
        }
        std::array<float, 512> guitarRaw {};
        std::array<float, 512> guitarLeft {};
        std::array<float, 512> guitarRight {};
        std::array<float, 512> vocalRaw {};
        std::array<float, 512> vocalLeft {};
        std::array<float, 512> vocalRight {};
        guitarRaw.fill(0.1f);
        guitarLeft.fill(0.2f);
        guitarRight.fill(0.21f);
        vocalRaw.fill(0.3f);
        vocalLeft.fill(0.4f);
        vocalRight.fill(0.41f);
        const std::array<const float*, 2> guitarProcessed {
            guitarLeft.data(), guitarRight.data() };
        const std::array<const float*, 2> vocalProcessed {
            vocalLeft.data(), vocalRight.data() };
        session->setCaptureRange(1000, 1256, 24000, 24256);
        session->setCaptureRange(1256, 1512, 24000, 24256);
        session->beginAudioTrackCapture("track:guitar", 1000, 24000);
        session->writeAudioTrack(
            "track:guitar", guitarRaw.data(), 256, guitarProcessed.data(), 256);
        session->endAudioTrackCapture("track:guitar", 1256, 24256);
        session->completeAudioTrackTail("track:guitar");
        session->beginAudioTrackCapture("track:guitar", 1256, 24000);
        const std::array<const float*, 2> guitarProcessedSecond {
            guitarLeft.data() + 256, guitarRight.data() + 256 };
        session->writeAudioTrack(
            "track:guitar", guitarRaw.data() + 256, 256, guitarProcessedSecond.data(), 256);
        session->endAudioTrackCapture("track:guitar", 1512, 24256);
        session->completeAudioTrackTail("track:guitar");
        session->beginAudioTrackCapture("track:vocal", 1000, 24000);
        session->writeAudioTrack(
            "track:vocal", vocalRaw.data(), 256, vocalProcessed.data(), 256);
        session->endAudioTrackCapture("track:vocal", 1256, 24256);
        session->completeAudioTrackTail("track:vocal");
        session->beginAudioTrackCapture("track:vocal", 1256, 24000);
        const std::array<const float*, 2> vocalProcessedSecond {
            vocalLeft.data() + 256, vocalRight.data() + 256 };
        session->writeAudioTrack(
            "track:vocal", vocalRaw.data() + 256, 256, vocalProcessedSecond.data(), 256);
        session->endAudioTrackCapture("track:vocal", 1512, 24256);
        session->completeAudioTrackTail("track:vocal");
        session->writeMidiTrack(
            "track:keys", "midi:keyboard",
            juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)), 1100);
        session->writeMidiTrack(
            "track:keys", "midi:keyboard",
            juce::MidiMessage::noteOn(1, 61, static_cast<juce::uint8>(100)), 900);
        session->writeMidiTrack(
            "track:keys", "midi:keyboard",
            juce::MidiMessage::noteOn(1, 62, static_cast<juce::uint8>(100)), 1600);
        session->markLoopBoundary(1256);
        written = session->finish(error);
    }
    const auto manifestText = directory.getChildFile("manifest.json").loadFileAsString();
    const auto mapped = manifestText.contains("\"trackKey\": \"0000\"")
        && manifestText.contains("\"trackId\": \"track:guitar\"")
        && manifestText.contains("\"pluginLatencySamples\": 8")
        && manifestText.contains("\"pluginTailSamples\": 16")
        && manifestText.contains("\"recordStartAudioSample\": 1000")
        && manifestText.contains("\"recordStartTimelineSample\": 24000")
        && manifestText.contains("\"captureId\": \"capture:self-test\"")
        && manifestText.contains("\"loopBoundariesSample\"")
        && manifestText.contains("\"captureSegments\"");
    const auto isolated = directory.getChildFile("tracks/0000/raw.wav").existsAsFile()
        && directory.getChildFile("tracks/0000/processed.wav").existsAsFile()
        && directory.getChildFile("tracks/0001/raw.wav").existsAsFile()
        && directory.getChildFile("tracks/0001/processed.wav").existsAsFile()
        && directory.getChildFile("tracks/0002/midi.json").existsAsFile();
    auto completedManifest = juce::JSON::parse(manifestText);
    const auto completedSegments = completedManifest.getProperty("captureSegments", {});
    const auto segmentOffsetsMapped = completedSegments.isArray()
        && completedSegments.size() == 2
        && static_cast<juce::int64>(
            completedSegments[0].getProperty("fileStartSample", -1)) == 0
        && static_cast<juce::int64>(
            completedSegments[0].getProperty("fileEndSample", -1)) == 256
        && static_cast<juce::int64>(
            completedSegments[1].getProperty("fileStartSample", -1)) == 256
        && static_cast<juce::int64>(
            completedSegments[1].getProperty("fileEndSample", -1)) == 512
        && static_cast<juce::int64>(
            completedSegments[0].getProperty("timelineStartSample", -1)) == 24000
        && static_cast<juce::int64>(
            completedSegments[1].getProperty("timelineStartSample", -1)) == 24000;
    const auto completedTracks = completedManifest.getProperty("tracks", {});
    const auto guitarSegments = completedTracks.isArray() && completedTracks.size() > 0
        ? completedTracks[0].getProperty("captureSegments", {})
        : juce::var {};
    const auto variantOffsetsMapped = guitarSegments.isArray()
        && guitarSegments.size() == 2
        && static_cast<juce::int64>(
            guitarSegments[0].getProperty("rawFileStartSample", -1)) == 0
        && static_cast<juce::int64>(
            guitarSegments[0].getProperty("rawFileEndSample", -1)) == 256
        && static_cast<juce::int64>(
            guitarSegments[0].getProperty("processedFileStartSample", -1)) == 0
        && static_cast<juce::int64>(
            guitarSegments[0].getProperty("processedFileEndSample", -1)) == 256
        && static_cast<juce::int64>(
            guitarSegments[1].getProperty("rawFileStartSample", -1)) == 256
        && static_cast<juce::int64>(
            guitarSegments[1].getProperty("processedFileStartSample", -1)) == 256;
    const auto cancelledDirectory = directory.getSiblingFile(
        directory.getFileName() + "-cancelled");
    auto cancelledSession = ArrangeRecordingSession::create(
        cancelledDirectory, configurationValue, error);
    const auto cancelledCleanly = cancelledSession != nullptr
        && cancelledSession->cancel(error)
        && !cancelledDirectory.exists();
    auto* result = new juce::DynamicObject();
    juce::Array<juce::var> checks;
    const auto addCheck = [&checks](const juce::String& name, const bool passed) {
        auto* check = new juce::DynamicObject();
        check->setProperty("name", name);
        check->setProperty("passed", passed);
        checks.add(juce::var(check));
    };
    addCheck("armed Tracks receive isolated Raw and Processed files", written && isolated);
    addCheck("manifest maps safe keys to stable Track IDs", mapped);
    addCheck("Loop capture maps Audio Clock and Timeline ranges to contiguous file offsets",
        segmentOffsetsMapped);
    addCheck("Raw and Processed Capture Segments retain independent file mappings",
        variantOffsetsMapped);
    const auto midiText =
        directory.getChildFile("tracks/0002/midi.json").loadFileAsString();
    addCheck("Punch filtering keeps only MIDI inside Capture Segments",
        midiText.contains("\"sampleOffset\": 100")
            && midiText.contains("\"sampleOffset\": 256")
            && midiText.contains("\"status\": 128")
            && !midiText.contains("\"data1\": 61")
            && !midiText.contains("\"data1\": 62"));
    addCheck("Count-in cancellation leaves no recoverable recording folder",
        cancelledCleanly);
    result->setProperty("type", "arrangeRecordingSelfTest");
    result->setProperty("checks", checks);
    result->setProperty("message", error);
    result->setProperty("passed", written && isolated && mapped && segmentOffsetsMapped
        && variantOffsetsMapped
        && cancelledCleanly
        && static_cast<bool>(checks[4].getProperty("passed", false)));
    return juce::var(result);
}

} // namespace riffra
