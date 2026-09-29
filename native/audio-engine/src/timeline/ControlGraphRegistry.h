#pragma once

#include <JuceHeader.h>

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

#include "PreparedTimeline.h"
#include "concurrency/RetireQueue.h"

namespace riffra {

/// Assigns every Track ID one key for the lifetime of the process.
///
/// The audio thread identifies tracks by key so realtime commands never carry
/// strings. Keys are never reused; 0 means "no track".
class TrackKeyRegistry final {
public:
    [[nodiscard]] std::uint32_t keyFor(const juce::String& trackId);

private:
    std::map<juce::String, std::uint32_t> keys;
    std::uint32_t nextKey = 1;
};

/// Control-side owner of every graph that is prepared, committed, or awaiting
/// destruction. The audio thread never touches the registry.
class ControlGraphRegistry final {
public:
    static constexpr std::size_t kRetireCapacity = 16;
    using Retired = RetireQueue<PreparedTimeline, kRetireCapacity>;

    struct State final {
        std::unique_ptr<PreparedTimeline> pending;
        /// The graph most recently committed; the audio thread may not have
        /// published it yet.
        PreparedTimeline* latestCommitted = nullptr;
        /// Every committed graph that has not been reclaimed, including the
        /// active one and the latest committed one.
        std::vector<std::unique_ptr<PreparedTimeline>> committed;
        std::uint64_t nextSerial = 1;
        std::uint64_t projectMeterEpoch = 0;
        /// Set when the audio device restarts; prepared device instances must
        /// then be rebuilt for the new device environment.
        bool devicesNeedReprepare = false;
        TrackKeyRegistry trackKeys;

        [[nodiscard]] PreparedTimeline* find(std::uint64_t serial) const noexcept;
    };

    /// Binds destruction of graphs to the calling thread.
    ControlGraphRegistry() noexcept;

    ControlGraphRegistry(const ControlGraphRegistry&) = delete;
    ControlGraphRegistry& operator=(const ControlGraphRegistry&) = delete;

    /// Runs `visit` with exclusive access to the registry state.
    template <typename Visit>
    decltype(auto) access(Visit&& visit) {
        const std::lock_guard lock(mutex);
        return visit(state);
    }

    template <typename Visit>
    decltype(auto) access(Visit&& visit) const {
        const std::lock_guard lock(mutex);
        return visit(static_cast<const State&>(state));
    }

    /// Destroys every graph the audio thread has retired and returns how many.
    ///
    /// Graphs own plugin instances, so they are destroyed only on the thread
    /// that constructed the registry; calls from any other thread do nothing.
    std::size_t reclaim(Retired& retired);

private:
    mutable std::mutex mutex;
    State state;
    std::thread::id reclaimThread;
};

}  // namespace riffra
