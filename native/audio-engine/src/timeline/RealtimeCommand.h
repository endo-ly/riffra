#pragma once

#include <cstdint>
#include <type_traits>

namespace riffra {

struct PreparedTimeline;
class ArrangementCaptureSink;

/// One request from a control thread to the owner of the realtime timeline state.
struct RealtimeCommand final {
    enum class Kind : std::uint8_t {
        play,
        stop,
        setStarting,
        seek,
        startRecording,
        stopRecording,
        stopArrangeRecording,
        panicAll,
        panicTrack,
        setLiveMidiTarget,
        targetedMidi,
        publishGraph,
        deviceStarted,
        setRecordingSink,
        clearRecordingSink,
    };

    Kind kind = Kind::play;
    /// Monotonic sequence assigned by the control side.
    std::uint64_t commandSequence = 0;
    /// `seek` target.
    std::uint64_t tick = 0;
    /// `startRecording` count-in length.
    std::int32_t countInBeats = 0;
    /// `panicTrack`, `setLiveMidiTarget` (0 clears the target) and `targetedMidi`.
    std::uint32_t trackKey = 0;
    /// `targetedMidi` message.
    std::uint8_t midiBytes[3] = {};
    std::uint8_t midiSize = 0;
    /// `publishGraph`.
    PreparedTimeline* graph = nullptr;
    ArrangementCaptureSink* recordingSink = nullptr;
};

static_assert(std::is_trivially_copyable_v<RealtimeCommand>);

/// Outcome of a control-side request that validates and then queues a realtime command.
enum class RealtimeRequest {
    accepted,
    /// The request is invalid for the current graph or state; see the error.
    rejected,
    /// The realtime command queue is full; the request may be retried.
    queueFull,
};

}  // namespace riffra
