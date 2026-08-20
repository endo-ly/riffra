#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <type_traits>

namespace riffra {

/// A fixed-capacity multi-producer, multi-consumer queue.
///
/// The queue never waits for another thread and never allocates after
/// construction. Operations make a bounded number of CAS attempts. A failed
/// push means that the queue was full or contended; callers own the overflow
/// policy for their domain.
template <typename T, std::size_t Capacity>
class BoundedMpmcQueue final {
    static_assert(Capacity >= 2, "BoundedMpmcQueue capacity must be at least two.");
    static_assert(std::is_trivially_copyable_v<T>,
                  "BoundedMpmcQueue items must be trivially copyable.");

    struct Cell final {
        std::atomic<std::size_t> sequence{0};
        T data{};
    };

public:
    static constexpr std::size_t kMaximumAttempts = 8;

    BoundedMpmcQueue() noexcept {
        for (std::size_t index = 0; index < Capacity; ++index)
            cells[index].sequence.store(index, std::memory_order_relaxed);
    }

    BoundedMpmcQueue(const BoundedMpmcQueue&) = delete;
    BoundedMpmcQueue& operator=(const BoundedMpmcQueue&) = delete;

    [[nodiscard]] bool tryPush(const T& value) noexcept {
        auto position = enqueuePosition.load(std::memory_order_relaxed);
        auto claimed = false;
        for (std::size_t attempt = 0; attempt < kMaximumAttempts; ++attempt) {
            auto& cell = cells[position % Capacity];
            const auto sequence = cell.sequence.load(std::memory_order_acquire);
            const auto difference =
                static_cast<std::intptr_t>(sequence) - static_cast<std::intptr_t>(position);
            if (difference == 0) {
                if (enqueuePosition.compare_exchange_weak(position, position + 1,
                                                          std::memory_order_relaxed)) {
                    claimed = true;
                    break;
                }
            } else if (difference < 0) {
                droppedPushesCount.fetch_add(1, std::memory_order_relaxed);
                return false;
            } else {
                position = enqueuePosition.load(std::memory_order_relaxed);
            }
        }
        if (!claimed) {
            droppedPushesCount.fetch_add(1, std::memory_order_relaxed);
            return false;
        }
        auto& cell = cells[position % Capacity];
        cell.data = value;
        cell.sequence.store(position + 1, std::memory_order_release);
        return true;
    }

    [[nodiscard]] bool tryPop(T& value) noexcept {
        return tryPopResult(value, kMaximumAttempts) == PopResult::value;
    }

    /// Drains from a producer-stopped, non-realtime boundary. Unlike tryPop,
    /// this retries contention until it can distinguish empty from a queued
    /// value, so finalizers cannot mistake a transient CAS failure for empty.
    [[nodiscard]] bool tryPopNonRealtime(T& value) noexcept {
        for (;;) {
            const auto result = tryPopResult(value, kMaximumAttempts);
            if (result == PopResult::value) return true;
            if (result == PopResult::empty) return false;
        }
    }

private:
    enum class PopResult { value, empty, contended };

    [[nodiscard]] PopResult tryPopResult(T& value, const std::size_t maximumAttempts) noexcept {
        auto position = dequeuePosition.load(std::memory_order_relaxed);
        auto claimed = false;
        for (std::size_t attempt = 0; attempt < maximumAttempts; ++attempt) {
            auto& cell = cells[position % Capacity];
            const auto sequence = cell.sequence.load(std::memory_order_acquire);
            const auto difference =
                static_cast<std::intptr_t>(sequence) - static_cast<std::intptr_t>(position + 1);
            if (difference == 0) {
                if (dequeuePosition.compare_exchange_weak(position, position + 1,
                                                          std::memory_order_relaxed)) {
                    claimed = true;
                    break;
                }
            } else if (difference < 0) {
                return PopResult::empty;
            } else {
                position = dequeuePosition.load(std::memory_order_relaxed);
            }
        }
        if (!claimed) return PopResult::contended;
        auto& cell = cells[position % Capacity];
        value = cell.data;
        cell.sequence.store(position + Capacity, std::memory_order_release);
        return PopResult::value;
    }

public:
    [[nodiscard]] std::uint64_t droppedPushes() const noexcept {
        return droppedPushesCount.load(std::memory_order_acquire);
    }

private:
    std::array<Cell, Capacity> cells{};
    alignas(64) std::atomic<std::size_t> enqueuePosition{0};
    alignas(64) std::atomic<std::size_t> dequeuePosition{0};
    std::atomic<std::uint64_t> droppedPushesCount{0};
};

}  // namespace riffra
