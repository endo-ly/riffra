#include <algorithm>
#include <utility>

#include "TimelineEngine.h"

namespace riffra {

bool TimelineEngine::setLiveMidiTarget(const juce::String& trackId, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (trackId.isNotEmpty()) {
        const auto isInstrumentTrack = [&trackId](const std::unique_ptr<Track>& track) {
            return track->id == trackId && track->runtime != nullptr &&
                   track->runtime->instrumentTrack;
        };
        const auto foundInTimeline =
            timeline != nullptr &&
            std::any_of(timeline->tracks.begin(), timeline->tracks.end(), isInstrumentTrack);
        const auto foundInPending = pendingTimeline != nullptr &&
                                    std::any_of(pendingTimeline->tracks.begin(),
                                                pendingTimeline->tracks.end(), isInstrumentTrack);
        if (!foundInTimeline && !foundInPending) {
            error = "Live MIDI target must be an Instrument Track.";
            return false;
        }
    }
    liveMidiTargetTrackId = trackId;
    const auto apply = [this](PreparedTimeline* prepared) {
        if (prepared == nullptr) return;
        for (auto& track : prepared->tracks) {
            auto& runtime = *track->runtime;
            runtime.setLowLatencyMonitoring(
                runtime.instrumentTrack ? (runtime.armed || track->id == liveMidiTargetTrackId)
                                        : runtime.monitorInput);
        }
    };
    apply(timeline.get());
    apply(pendingTimeline.get());
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::enqueueLiveMidi(const juce::MidiMessage& message,
                                     const juce::String& deviceId) noexcept {
    if (!armedInstrumentTrack.load(std::memory_order_acquire)) return false;
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) return true;
    for (auto& trackPtr : timeline->tracks) {
        auto& track = *trackPtr;
        if (track.runtime->instrumentTrack && track.runtime->armed &&
            ArrangementGraph::midiRouteMatches(track.runtime->midiDeviceId,
                                               track.runtime->midiChannel, deviceId,
                                               message.getChannel())) {
            if (track.runtime != nullptr && track.runtime->hasLoadedInstrument())
                (void)track.runtime->enqueueMidi(message);
            if (recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
                recordingCapture->writeMidiTrack(track.id, deviceId, message,
                                                 audioClockSample.load(std::memory_order_acquire));
            }
        }
    }
    return true;
}

bool TimelineEngine::enqueueTargetedMidi(const juce::String& trackId,
                                         const juce::MidiMessage& message,
                                         juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI input.";
        return false;
    }
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) {
        error = "The Arrangement Graph is unavailable for targeted MIDI.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "The target Track is not available in the Arrangement Graph.";
        return false;
    }
    auto& track = **found;
    if (track.runtime == nullptr || !track.runtime->instrumentTrack ||
        !track.runtime->hasLoadedInstrument()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    if (!track.runtime->enqueueMidi(message)) {
        error = "The target Instrument Track could not queue MIDI.";
        return false;
    }
    if (track.runtime->armed &&
        recordingPhase.load(std::memory_order_acquire) == RecordingPhase::recording) {
        recordingCapture->writeMidiTrack(track.id, "riffra:play-surface", message,
                                         audioClockSample.load(std::memory_order_acquire));
    }
    return true;
}

bool TimelineEngine::panicTargetedMidi(const juce::String& trackId, juce::String& error) noexcept {
    if (trackId.isEmpty()) {
        error = "A target track is required for MIDI panic.";
        return false;
    }
    const juce::SpinLock::ScopedTryLockType lock(timelineLock);
    if (!lock.isLocked() || timeline == nullptr) {
        error = "The Arrangement Graph is unavailable for targeted MIDI panic.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "The target Track is not available in the Arrangement Graph.";
        return false;
    }
    auto& track = **found;
    if (track.runtime == nullptr || !track.runtime->instrumentTrack ||
        !track.runtime->hasLoadedInstrument()) {
        error = "The target Instrument Track has no loaded instrument.";
        return false;
    }
    track.runtime->panic();
    return true;
}

void TimelineEngine::panicAllInstrumentTracks() noexcept {
    panicAllPending.store(true, std::memory_order_release);
}

void TimelineEngine::servicePendingPanic() noexcept {
    AudioReadScope activeRead(*this);
    if (auto* active = activeRead.get(); active != nullptr) applyPendingPanic(*active);
}

PluginRack* TimelineEngine::findDevice(const juce::String& trackId,
                                       const juce::String& deviceId) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) return nullptr;
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) return nullptr;
    auto& track = **found;
    if (track.runtime != nullptr && track.runtime->instrument() != nullptr &&
        deviceId == track.instrumentDeviceId)
        return track.runtime->instrument()->vst3Rack();
    return track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
}

juce::var TimelineEngine::deviceStatus(const juce::String& trackId, const juce::String& deviceId,
                                       juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin status."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->status();
    if (!result.isObject()) {
        error = "Plugin status was not an object.";
        return {};
    }
    auto* object = result.getDynamicObject();
    object->setProperty("type", "trackDeviceStatus");
    object->setProperty("id", deviceId);
    object->setProperty("source", "vst3");
    object->setProperty("parameterCount", static_cast<int>(rack->parameterCount()));
    object->setProperty("statePersisted", true);
    auto* capabilities = new juce::DynamicObject();
    capabilities->setProperty("parameters", rack->parameterCount() > 0);
    capabilities->setProperty("state", true);
    capabilities->setProperty("presets", rack->hasPrograms());
    capabilities->setProperty("editor", rack->hasEditor());
    object->setProperty("capabilities", juce::var(capabilities));
    return result;
}

juce::var TimelineEngine::deviceParameterStatus(const juce::String& trackId,
                                                const juce::String& deviceId,
                                                juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin parameters."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->parameterStatus();
    if (!result.isObject()) {
        error = "Plugin parameter status was not an object.";
        return {};
    }
    result.getDynamicObject()->setProperty("type", "trackDeviceParameters");
    return result;
}

juce::var TimelineEngine::deviceProgramStatus(const juce::String& trackId,
                                              const juce::String& deviceId,
                                              juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    const auto isInstrument = track.runtime != nullptr && track.runtime->instrumentTrack &&
                              track.instrumentDeviceId == deviceId;
    const auto* rack =
        isInstrument
            ? (track.runtime != nullptr && track.runtime->instrument() != nullptr
                   ? track.runtime->instrument()->vst3Rack()
                   : nullptr)
            : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr);
    if (rack == nullptr) {
        error = isInstrument ? "Built-in instruments do not expose plugin programs."
                             : "Track Device was not found.";
        return {};
    }
    auto result = rack->programStatus();
    if (!result.isObject()) {
        error = "Plugin program status was not an object.";
        return {};
    }
    result.getDynamicObject()->setProperty("type", "trackDevicePrograms");
    return result;
}

bool TimelineEngine::mirrorEditorDeviceState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const juce::var& persistedState,
                                             juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                               ? track.runtime->instrument()->vst3Rack()
                               : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not provide a VST3 editor state.";
            return false;
        }
        return instrument->applyPersistedState(persistedState, error);
    }
    auto* effect =
        track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
    if (effect == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    return effect->applyPersistedState(persistedState, error);
}

bool TimelineEngine::mirrorEditorDeviceParameter(const juce::String& trackId,
                                                 const juce::String& deviceId,
                                                 const int parameterIndex, const float value,
                                                 juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                               ? track.runtime->instrument()->vst3Rack()
                               : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not expose editable parameters.";
            return false;
        }
        instrument->enqueueParameterChange(parameterIndex, value);
        sequence.fetch_add(1, std::memory_order_relaxed);
        return true;
    }
    auto* effect =
        track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
    if (effect == nullptr) {
        error = "Track Device was not found.";
        return false;
    }
    effect->enqueueParameterChange(parameterIndex, value);
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

juce::var TimelineEngine::devicePersistedState(const juce::String& trackId,
                                               const juce::String& deviceId,
                                               juce::String& error) const {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Timeline is not loaded.";
        return {};
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return {};
    }
    const auto& track = **found;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        const auto* instrument = track.runtime != nullptr && track.runtime->instrument() != nullptr
                                     ? track.runtime->instrument()->vst3Rack()
                                     : nullptr;
        if (instrument == nullptr) {
            error = "Built-in instruments do not provide persisted VST3 state.";
            return {};
        }
        return instrument->persistedState(error);
    }
    return track.runtime != nullptr ? track.runtime->effects().persistedState(deviceId, error)
                                    : juce::var();
}

bool TimelineEngine::preparedTrackReusesRuntimeDevices(const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (pendingTimeline == nullptr) return false;
    const auto track = std::find_if(pendingTimeline->tracks.begin(), pendingTimeline->tracks.end(),
                                    [&trackId](const auto& item) { return item->id == trackId; });
    return track != pendingTimeline->tracks.end() && (*track)->reuseRuntimeDevices;
}

bool TimelineEngine::hasPreparedSnapshot() const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return pendingTimeline != nullptr;
}

bool TimelineEngine::setDeviceBypassed(const juce::String& trackId, const juce::String& deviceId,
                                       const bool bypassed, juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime == nullptr || track.runtime->instrument() == nullptr) {
            error = "Instrument runtime is not loaded.";
            return false;
        }
        track.runtime->instrument()->setBypassed(bypassed);
    } else {
        auto* effect =
            track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId) : nullptr;
        if (effect == nullptr) {
            error = "Track Device was not found.";
            return false;
        }
        effect->setBypassed(bypassed);
    }
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceParameter(const juce::String& trackId, const juce::String& deviceId,
                                        const int parameterIndex, const float value,
                                        juce::String& error) noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    const auto isInstrumentDevice = track.runtime != nullptr && track.runtime->instrumentTrack &&
                                    track.instrumentDeviceId == deviceId;
    auto* playback =
        isInstrumentDevice && track.runtime != nullptr && track.runtime->instrument() != nullptr
            ? track.runtime->instrument()->vst3Rack()
            : (isInstrumentDevice
                   ? nullptr
                   : (track.runtime != nullptr ? track.runtime->effects().findDevice(deviceId)
                                               : nullptr));
    if (playback == nullptr) {
        error = isInstrumentDevice ? "Built-in instruments do not expose editable parameters."
                                   : "Track Device was not found.";
        return false;
    }
    const auto parameterStatus = playback->parameterStatus().getProperty("parameters", {});
    if (!parameterStatus.isArray() || parameterIndex < 0 ||
        parameterIndex >= parameterStatus.size()) {
        error = "Track Device parameter index is invalid.";
        return false;
    }
    if (!playback->setParameter(parameterIndex, value, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDevicePersistedState(const juce::String& trackId,
                                             const juce::String& deviceId,
                                             const juce::var& persistedState, juce::String& error) {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    PluginRack* target = nullptr;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime != nullptr && track.runtime->instrument() != nullptr)
            target = track.runtime->instrument()->vst3Rack();
    } else {
        if (track.runtime != nullptr) target = track.runtime->effects().findDevice(deviceId);
    }
    if (target == nullptr) {
        error = track.runtime != nullptr && track.runtime->instrumentTrack &&
                        track.instrumentDeviceId == deviceId
                    ? "Built-in instruments do not expose plugin state."
                    : "Track Device was not found.";
        return false;
    }
    if (!target->applyPersistedState(persistedState, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::setDeviceProgram(const juce::String& trackId, const juce::String& deviceId,
                                      const int programIndex, juce::String& error) {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    if (timeline == nullptr) {
        error = "Arrangement Graph is not loaded.";
        return false;
    }
    const auto found = std::find_if(timeline->tracks.begin(), timeline->tracks.end(),
                                    [&](const auto& track) { return track->id == trackId; });
    if (found == timeline->tracks.end()) {
        error = "Track was not found.";
        return false;
    }
    auto& track = **found;
    PluginRack* target = nullptr;
    if (track.runtime != nullptr && track.runtime->instrumentTrack &&
        track.instrumentDeviceId == deviceId) {
        if (track.runtime != nullptr && track.runtime->instrument() != nullptr)
            target = track.runtime->instrument()->vst3Rack();
    } else {
        if (track.runtime != nullptr) target = track.runtime->effects().findDevice(deviceId);
    }
    if (target == nullptr) {
        error = track.runtime != nullptr && track.runtime->instrumentTrack &&
                        track.instrumentDeviceId == deviceId
                    ? "Built-in instruments do not expose plugin programs."
                    : "Track Device was not found.";
        return false;
    }
    const auto status = target->programStatus();
    if (!status.isObject()) {
        error = "Plugin program status was not an object.";
        return false;
    }
    const auto enumerationError = status.getProperty("error", {});
    if (!enumerationError.isVoid()) {
        error = enumerationError.toString();
        return false;
    }
    const auto programs = status.getProperty("programs", {});
    if (!programs.isArray() || programIndex < 0 || programIndex >= programs.size()) {
        error = "Plugin program index is out of range.";
        return false;
    }
    if (!target->setProgram(programIndex, error)) return false;
    sequence.fetch_add(1, std::memory_order_relaxed);
    return true;
}

bool TimelineEngine::monitoringEnabled() const noexcept {
    return monitorLiveInput.load(std::memory_order_acquire);
}

bool TimelineEngine::isLiveMidiTarget(const juce::String& trackId) const noexcept {
    const juce::SpinLock::ScopedLockType lock(timelineLock);
    return liveMidiTargetTrackId == trackId;
}

bool TimelineEngine::monitoringInputChannel(const int channel) const noexcept {
    if (channel < 0 || channel >= 32) return false;
    const auto channels = monitoringInputChannels.load(std::memory_order_acquire);
    return (channels & (std::uint32_t{1} << static_cast<unsigned>(channel))) != 0;
}

}  // namespace riffra
