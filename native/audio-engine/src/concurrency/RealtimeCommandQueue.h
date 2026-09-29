#pragma once

#include <cstddef>
#include <mutex>

#include "BoundedMpmcQueue.h"

namespace riffra {

/// Carries commands from control threads to the single realtime consumer.
///
/// Producers are serialized by a mutex, so a push fails only when the queue is
/// full. The consumer never takes the mutex and drains every queued command in
/// submission order.
template <typename Command, std::size_t Capacity>
class RealtimeCommandQueue final {
public:
    RealtimeCommandQueue() = default;
    RealtimeCommandQueue(const RealtimeCommandQueue&) = delete;
    RealtimeCommandQueue& operator=(const RealtimeCommandQueue&) = delete;

    /// Control thread only. Returns false when the queue is full.
    [[nodiscard]] bool tryPush(const Command& command) {
        const std::lock_guard lock(producerMutex);
        return queue.tryPush(command);
    }

    /// Consumer only. Applies every queued command in submission order.
    template <typename Apply>
    void drain(Apply&& apply) noexcept {
        Command command{};
        while (queue.tryPop(command)) apply(command);
    }

private:
    std::mutex producerMutex;
    BoundedMpmcQueue<Command, Capacity> queue;
};

}  // namespace riffra
