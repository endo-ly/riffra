#pragma once

#include <cstdint>

namespace riffra {

enum class TransportState : std::uint8_t { stopped, starting, playing, faulted };
enum class RecordingPhase : std::uint8_t { idle, countingIn, recording, stopping };

/// Realtime state published by the owner of the timeline at the end of each block.
///
/// Readers receive one consistent frame through `SeqLockFrame`; no field is read
/// from a partially applied block.
struct RealtimeFrame final {
    /// Sequence of the last realtime command applied by the owner.
    std::uint64_t appliedCommandSequence = 0;
    /// Registry serial of the active graph; 0 before the first publication.
    std::uint64_t activeGraphSerial = 0;
    std::uint64_t graphRevision = 0;
    TransportState transportState = TransportState::stopped;
    RecordingPhase recordingPhase = RecordingPhase::idle;
    /// Playback position; while a seek is pending this is the seek target.
    std::int64_t timelineSample = 0;
    std::uint64_t audioClockSample = 0;
    std::uint64_t recordingStartTick = 0;
    std::uint32_t recordingPassOrdinal = 0;
    std::uint64_t clockGeneration = 0;
    std::uint64_t discontinuity = 0;
    std::uint64_t graphPublishCount = 0;
    double sampleRate = 0.0;
    /// Track key of the Play Surface target; 0 when there is none.
    std::uint32_t liveMidiTargetTrackKey = 0;
    /// Whether the active graph routes live MIDI input to an armed Instrument Track.
    bool armedInstrumentTrack = false;
};

}  // namespace riffra
