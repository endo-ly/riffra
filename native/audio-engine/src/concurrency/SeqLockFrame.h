#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <type_traits>

namespace riffra {

/// Publishes a trivially copyable frame from one writer to any number of readers.
///
/// Every word of the frame is an atomic, so a reader racing the writer never
/// performs a data race; the sequence number tells the reader whether the words
/// it loaded belong to one publication. Only one thread writes at a time.
template <typename Frame>
class SeqLockFrame final {
    static_assert(std::is_trivially_copyable_v<Frame>, "SeqLockFrame requires a trivial frame.");
    static constexpr std::size_t kWordCount =
        (sizeof(Frame) + sizeof(std::uint64_t) - 1) / sizeof(std::uint64_t);

public:
    SeqLockFrame() noexcept { write(Frame{}); }
    SeqLockFrame(const SeqLockFrame&) = delete;
    SeqLockFrame& operator=(const SeqLockFrame&) = delete;

    /// Writer only. Never blocks.
    void write(const Frame& frame) noexcept {
        std::array<std::uint64_t, kWordCount> source{};
        std::memcpy(source.data(), &frame, sizeof(Frame));
        const auto begin = sequence.load(std::memory_order_relaxed) + 1;
        sequence.store(begin, std::memory_order_relaxed);
        std::atomic_thread_fence(std::memory_order_release);
        for (std::size_t index = 0; index < kWordCount; ++index)
            words[index].store(source[index], std::memory_order_relaxed);
        sequence.store(begin + 1, std::memory_order_release);
    }

    /// Any thread. Retries while a publication is in progress.
    [[nodiscard]] Frame read() const noexcept {
        std::array<std::uint64_t, kWordCount> copy{};
        for (;;) {
            const auto before = sequence.load(std::memory_order_acquire);
            if ((before & 1u) != 0) continue;
            for (std::size_t index = 0; index < kWordCount; ++index)
                copy[index] = words[index].load(std::memory_order_relaxed);
            std::atomic_thread_fence(std::memory_order_acquire);
            if (sequence.load(std::memory_order_relaxed) == before) break;
        }
        Frame frame;
        std::memcpy(&frame, copy.data(), sizeof(Frame));
        return frame;
    }

private:
    std::atomic<std::uint64_t> sequence{0};
    std::array<std::atomic<std::uint64_t>, kWordCount> words{};
};

}  // namespace riffra
