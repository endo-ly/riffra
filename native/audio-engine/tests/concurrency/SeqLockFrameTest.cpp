#include <gtest/gtest.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <thread>

#include "concurrency/SeqLockFrame.h"

namespace riffra {
namespace {

struct PairedFrame final {
    std::uint64_t a = 0;
    double middle = 0.0;
    std::uint64_t b = 0;
};

TEST(SeqLockFrameTest, ReadersNeverObserveATornFrame) {
    // Arrange
    SeqLockFrame<PairedFrame> frame;
    std::atomic<bool> writing{true};
    std::array<std::atomic<std::uint64_t>, 2> tornReads{};
    std::array<std::atomic<std::uint64_t>, 2> reads{};

    // Act
    std::array<std::thread, 2> readers;
    for (std::size_t reader = 0; reader < readers.size(); ++reader) {
        readers[reader] = std::thread([&, reader] {
            while (writing.load(std::memory_order_acquire)) {
                const auto value = frame.read();
                if (value.a != value.b || value.middle != static_cast<double>(value.a))
                    tornReads[reader].fetch_add(1, std::memory_order_relaxed);
                reads[reader].fetch_add(1, std::memory_order_relaxed);
            }
        });
    }
    while (reads[0].load(std::memory_order_relaxed) == 0 ||
           reads[1].load(std::memory_order_relaxed) == 0)
        std::this_thread::yield();
    for (std::uint64_t value = 1; value <= 200'000; ++value)
        frame.write({value, static_cast<double>(value), value});
    writing.store(false, std::memory_order_release);
    for (auto& reader : readers) reader.join();

    // Assert
    for (std::size_t reader = 0; reader < readers.size(); ++reader) {
        EXPECT_EQ(tornReads[reader].load(), 0u);
        EXPECT_GT(reads[reader].load(), 0u);
    }
    EXPECT_EQ(frame.read().a, 200'000u);
}

}  // namespace
}  // namespace riffra
