#pragma once

#include <cstddef>

#include "BoundedMpmcQueue.h"

namespace riffra {

/// Returns objects that the realtime thread has stopped referencing.
///
/// The realtime thread is the only producer and one control thread is the only
/// consumer. The producer owns the capacity contract: it must never have more
/// objects outstanding than `Capacity`.
template <typename T, std::size_t Capacity>
class RetireQueue final {
public:
    RetireQueue() = default;
    RetireQueue(const RetireQueue&) = delete;
    RetireQueue& operator=(const RetireQueue&) = delete;

    /// Realtime producer only. Returns false when the capacity contract was broken.
    [[nodiscard]] bool retire(T* retired) noexcept { return queue.tryPush(retired); }

    /// Control consumer only. Hands every retired object to `reclaim`.
    template <typename Reclaim>
    void reclaim(Reclaim&& reclaimRetired) {
        T* retired = nullptr;
        while (queue.tryPopNonRealtime(retired)) reclaimRetired(retired);
    }

    static constexpr std::size_t capacity() noexcept { return Capacity; }

private:
    BoundedMpmcQueue<T*, Capacity> queue;
};

}  // namespace riffra
