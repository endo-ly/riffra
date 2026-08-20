#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <thread>
#include <vector>

#include "BoundedMpmcQueue.h"

namespace riffra {
namespace {

TEST(BoundedMpmcQueueTest, ReportsOverflowAndPreservesCapacity) {
    BoundedMpmcQueue<int, 8> queue;

    for (int value = 0; value < 8; ++value) EXPECT_TRUE(queue.tryPush(value));
    EXPECT_FALSE(queue.tryPush(8));
    EXPECT_EQ(queue.droppedPushes(), 1u);

    for (int expected = 0; expected < 8; ++expected) {
        int value = -1;
        ASSERT_TRUE(queue.tryPop(value));
        EXPECT_EQ(value, expected);
    }
    int value = -1;
    EXPECT_FALSE(queue.tryPop(value));
}

TEST(BoundedMpmcQueueTest, AcceptsConcurrentProducers) {
    constexpr int producerCount = 4;
    constexpr int valuesPerProducer = 1000;
    BoundedMpmcQueue<int, producerCount * valuesPerProducer> queue;
    std::array<std::thread, producerCount> producers;
    std::array<std::atomic<bool>, producerCount * valuesPerProducer> accepted{};

    for (int producer = 0; producer < producerCount; ++producer) {
        producers[static_cast<std::size_t>(producer)] = std::thread([&queue, &accepted, producer] {
            for (int value = 0; value < valuesPerProducer; ++value) {
                const auto item = producer * valuesPerProducer + value;
                accepted[static_cast<std::size_t>(item)].store(queue.tryPush(item),
                                                               std::memory_order_release);
            }
        });
    }
    for (auto& producer : producers) producer.join();

    std::vector<int> expected;
    expected.reserve(producerCount * valuesPerProducer);
    for (int item = 0; item < producerCount * valuesPerProducer; ++item) {
        if (accepted[static_cast<std::size_t>(item)].load(std::memory_order_acquire))
            expected.push_back(item);
    }

    std::vector<int> values;
    values.reserve(expected.size());
    int value = 0;
    while (queue.tryPopNonRealtime(value)) values.push_back(value);
    std::sort(values.begin(), values.end());
    std::sort(expected.begin(), expected.end());

    EXPECT_EQ(queue.droppedPushes(),
              static_cast<std::uint64_t>(producerCount * valuesPerProducer - expected.size()));
    EXPECT_EQ(values, expected);
}

}  // namespace
}  // namespace riffra
