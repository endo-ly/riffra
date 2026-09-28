#include <gtest/gtest.h>

#include <array>
#include <thread>
#include <vector>

#include "concurrency/RealtimeCommandQueue.h"

namespace riffra {
namespace {

TEST(RealtimeCommandQueueTest, RejectsOnlyWhenFull) {
    // Arrange
    RealtimeCommandQueue<int, 4> queue;
    for (int value = 0; value < 4; ++value) ASSERT_TRUE(queue.tryPush(value));

    // Act
    const auto overflowAccepted = queue.tryPush(4);
    std::vector<int> drained;
    queue.drain([&drained](const int value) { drained.push_back(value); });

    // Assert
    EXPECT_FALSE(overflowAccepted);
    EXPECT_EQ(drained, (std::vector<int>{0, 1, 2, 3}));
}

TEST(RealtimeCommandQueueTest, ContendedProducersNeverLoseCommandsWithinCapacity) {
    // Arrange
    constexpr int producerCount = 8;
    constexpr int commandsPerProducer = 64;
    RealtimeCommandQueue<int, producerCount * commandsPerProducer> queue;
    std::array<std::thread, producerCount> producers;
    std::array<int, producerCount> rejected{};

    // Act
    for (int producer = 0; producer < producerCount; ++producer) {
        producers[static_cast<std::size_t>(producer)] = std::thread([&, producer] {
            for (int command = 0; command < commandsPerProducer; ++command)
                if (!queue.tryPush(producer * commandsPerProducer + command))
                    ++rejected[static_cast<std::size_t>(producer)];
        });
    }
    for (auto& producer : producers) producer.join();
    std::vector<int> lastByProducer(producerCount, -1);
    auto ordered = true;
    auto drainedCount = 0;
    queue.drain([&](const int value) {
        const auto producer = value / commandsPerProducer;
        ordered = ordered && value > lastByProducer[static_cast<std::size_t>(producer)];
        lastByProducer[static_cast<std::size_t>(producer)] = value;
        ++drainedCount;
    });

    // Assert
    EXPECT_EQ(rejected, (std::array<int, producerCount>{}));
    EXPECT_EQ(drainedCount, producerCount * commandsPerProducer);
    EXPECT_TRUE(ordered);
}

}  // namespace
}  // namespace riffra
