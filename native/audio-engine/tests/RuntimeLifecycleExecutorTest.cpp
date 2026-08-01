#include <gtest/gtest.h>

#include "RuntimeLifecycleExecutor.h"

#include <atomic>
#include <chrono>
#include <mutex>
#include <thread>
#include <vector>

namespace riffra {

TEST(RuntimeLifecycleExecutorTest, ExecutesSubmittedTasksInOrder) {
    RuntimeLifecycleExecutor executor;
    std::mutex valuesLock;
    std::vector<int> values;

    for (int value = 1; value <= 3; ++value) {
        ASSERT_TRUE(executor.submit([&, value] {
            const std::lock_guard lock(valuesLock);
            values.push_back(value);
        }));
    }

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    const std::lock_guard lock(valuesLock);
    EXPECT_EQ(values, (std::vector<int> { 1, 2, 3 }));
}

TEST(RuntimeLifecycleExecutorTest, NeverRunsTwoLifecycleTasksConcurrently) {
    RuntimeLifecycleExecutor executor;
    std::atomic<int> active { 0 };
    std::atomic<int> maximum { 0 };

    for (int index = 0; index < 8; ++index) {
        ASSERT_TRUE(executor.submit([&] {
            const auto current = active.fetch_add(1, std::memory_order_acq_rel) + 1;
            auto observed = maximum.load(std::memory_order_acquire);
            while (current > observed
                   && !maximum.compare_exchange_weak(
                       observed, current, std::memory_order_acq_rel)) {
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
            active.fetch_sub(1, std::memory_order_acq_rel);
        }));
    }

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(maximum.load(std::memory_order_acquire), 1);
}

TEST(RuntimeLifecycleExecutorTest, StopRejectsNewLifecycleTasks) {
    RuntimeLifecycleExecutor executor;
    executor.requestStop();
    EXPECT_FALSE(executor.submit([] {}));
    EXPECT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
}

TEST(RuntimeLifecycleExecutorTest, CoalescesLatestStateTaskByKey) {
    RuntimeLifecycleExecutor executor;
    std::atomic<bool> blockerStarted { false };
    std::atomic<bool> releaseBlocker { false };
    std::atomic<int> executions { 0 };
    std::atomic<int> value { 0 };

    ASSERT_TRUE(executor.submit([&] {
        blockerStarted.store(true, std::memory_order_release);
        while (!releaseBlocker.load(std::memory_order_acquire))
            std::this_thread::yield();
    }));
    while (!blockerStarted.load(std::memory_order_acquire))
        std::this_thread::yield();

    ASSERT_EQ(
        executor.submitState("track/device/parameter/7", [&] {
        executions.fetch_add(1, std::memory_order_acq_rel);
        value.store(1, std::memory_order_release);
        }),
        RuntimeLifecycleExecutor::StateSubmitResult::accepted);
    ASSERT_EQ(
        executor.submitState("track/device/parameter/7", [&] {
        executions.fetch_add(1, std::memory_order_acq_rel);
        value.store(2, std::memory_order_release);
        }),
        RuntimeLifecycleExecutor::StateSubmitResult::coalesced);
    releaseBlocker.store(true, std::memory_order_release);

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(executions.load(std::memory_order_acquire), 1);
    EXPECT_EQ(value.load(std::memory_order_acquire), 2);
}

TEST(RuntimeLifecycleExecutorTest, DropsOldestStateWhenCapacityIsExceeded) {
    RuntimeLifecycleExecutor executor;
    std::atomic<bool> blockerStarted { false };
    std::atomic<bool> releaseBlocker { false };
    std::atomic<int> newestValue { 0 };

    ASSERT_TRUE(executor.submit([&] {
        blockerStarted.store(true, std::memory_order_release);
        while (!releaseBlocker.load(std::memory_order_acquire))
            std::this_thread::yield();
    }));
    while (!blockerStarted.load(std::memory_order_acquire))
        std::this_thread::yield();

    for (int index = 0; index < 256; ++index) {
        EXPECT_EQ(
            executor.submitState("state-" + std::to_string(index), [&, index] {
                newestValue.store(index, std::memory_order_release);
            }),
            RuntimeLifecycleExecutor::StateSubmitResult::accepted);
    }
    EXPECT_EQ(
        executor.submitState("state-newest", [&] {
            newestValue.store(999, std::memory_order_release);
        }),
        RuntimeLifecycleExecutor::StateSubmitResult::droppedCapacity);

    releaseBlocker.store(true, std::memory_order_release);
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(newestValue.load(std::memory_order_acquire), 999);
}

}  // namespace riffra
