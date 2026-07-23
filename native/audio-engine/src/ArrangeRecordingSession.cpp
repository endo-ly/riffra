#include "ArrangeRecordingSession.h"

#include <algorithm>
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

void ArrangeRecordingSession::writeAudioTrack(
    const juce::String& trackId,
    const float* raw,
    const float* const* processed,
    const int sampleCount) noexcept {
    if (finished.load(std::memory_order_acquire)
        || raw == nullptr || processed == nullptr || sampleCount <= 0)
        return;
    const auto found = std::find_if(tracks.begin(), tracks.end(), [&](const TrackWriter& track) {
        return track.trackId == trackId;
    });
    if (found == tracks.end() || found->audio == nullptr)
        return;
    const std::array<const float*, 1> rawChannels { raw };
    (void) found->audio->write(rawChannels.data(), processed, sampleCount);
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
    const std::uint64_t endAudioSample) noexcept {
    auto currentStart = recordStartAudioSample.load(std::memory_order_acquire);
    while (startAudioSample < currentStart
           && !recordStartAudioSample.compare_exchange_weak(
               currentStart, startAudioSample, std::memory_order_acq_rel))
        {}
    auto currentEnd = recordEndAudioSample.load(std::memory_order_acquire);
    while (endAudioSample > currentEnd
           && !recordEndAudioSample.compare_exchange_weak(
               currentEnd, endAudioSample, std::memory_order_acq_rel))
        {}
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
                for (const auto& event : track.midiEvents) {
                    auto* value = new juce::DynamicObject();
                    value->setProperty("sampleOffset", static_cast<juce::int64>(
                        event.audioSample >= recordStartAudioSample.load(std::memory_order_acquire)
                            ? event.audioSample
                                - recordStartAudioSample.load(std::memory_order_acquire)
                            : 0));
                    value->setProperty("sourceDeviceId", event.sourceDeviceId);
                    value->setProperty("status", event.status);
                    value->setProperty("channel", event.channel);
                    value->setProperty("data1", event.data1);
                    value->setProperty("data2", event.data2);
                    events.add(juce::var(value));
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

juce::var ArrangeRecordingSession::status() const {
    auto* result = new juce::DynamicObject();
    result->setProperty("active", !finished.load(std::memory_order_acquire));
    result->setProperty("directory", directory.getFullPathName());
    result->setProperty("sampleRate", sampleRate);
    std::uint64_t written = 0;
    std::uint64_t dropped = 0;
    for (const auto& track : tracks) {
        if (track.audio != nullptr) {
            written = std::max(written, track.audio->getSamplesWritten());
            dropped += track.audio->getDroppedBlocks();
        }
    }
    result->setProperty("samplesWritten", static_cast<juce::int64>(written));
    result->setProperty("droppedBlocks", static_cast<juce::int64>(dropped));
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
    root->setProperty("timelineStartTick", static_cast<juce::int64>(timelineStartTick));
    std::uint64_t samplesWritten = 0;
    std::uint64_t droppedBlocks = 0;
    std::uint64_t missingSamples = 0;
    for (const auto& track : tracks) {
        if (track.audio != nullptr) {
            samplesWritten = std::max(samplesWritten, track.audio->getSamplesWritten());
            droppedBlocks += track.audio->getDroppedBlocks();
            missingSamples += track.audio->getMissingSamples();
        }
    }
    root->setProperty("samplesWritten", static_cast<juce::int64>(samplesWritten));
    root->setProperty("droppedBlocks", static_cast<juce::int64>(droppedBlocks));
    root->setProperty("missingSamples", static_cast<juce::int64>(missingSamples));
    root->setProperty("recoveryStatus", droppedBlocks == 0 ? "clean" : "partial");
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
        if (track.kind == "audio") {
            value->setProperty("rawFile", "tracks/" + track.trackKey + "/raw.wav");
            value->setProperty(
                "processedFile", "tracks/" + track.trackKey + "/processed.wav");
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
        tracks.add(juce::var(track));
    };
    addTrack("track:guitar", "audio", 0);
    addTrack("track:vocal", "audio", 1);
    addTrack("track:keys", "instrument", -1);
    configuration->setProperty("tracks", tracks);
    juce::String error;
    auto session = ArrangeRecordingSession::create(
        directory, juce::var(configuration), error);
    bool written = session != nullptr;
    if (session != nullptr) {
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
        session->setCaptureRange(1000, 1512);
        session->writeAudioTrack(
            "track:guitar", guitarRaw.data(), guitarProcessed.data(), 512);
        session->writeAudioTrack(
            "track:vocal", vocalRaw.data(), vocalProcessed.data(), 512);
        session->writeMidiTrack(
            "track:keys", "midi:keyboard",
            juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)), 1100);
        session->markLoopBoundary(1256);
        written = session->finish(error);
    }
    const auto manifestText = directory.getChildFile("manifest.json").loadFileAsString();
    const auto mapped = manifestText.contains("\"trackKey\": \"0000\"")
        && manifestText.contains("\"trackId\": \"track:guitar\"")
        && manifestText.contains("\"recordStartAudioSample\": 1000")
        && manifestText.contains("\"loopBoundariesSample\"");
    const auto isolated = directory.getChildFile("tracks/0000/raw.wav").existsAsFile()
        && directory.getChildFile("tracks/0000/processed.wav").existsAsFile()
        && directory.getChildFile("tracks/0001/raw.wav").existsAsFile()
        && directory.getChildFile("tracks/0001/processed.wav").existsAsFile()
        && directory.getChildFile("tracks/0002/midi.json").existsAsFile();
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
    addCheck("MIDI events use Native Audio Sample offsets",
        directory.getChildFile("tracks/0002/midi.json")
            .loadFileAsString().contains("\"sampleOffset\": 100"));
    result->setProperty("type", "arrangeRecordingSelfTest");
    result->setProperty("checks", checks);
    result->setProperty("message", error);
    result->setProperty("passed", written && isolated && mapped
        && static_cast<bool>(checks[2].getProperty("passed", false)));
    return juce::var(result);
}

} // namespace riffra
