#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <mutex>
#include <thread>
#include <vector>

#include "RuntimeLifecycleExecutor.h"

namespace riffra {

namespace {

bool waitForFlag(const std::atomic<bool>& flag, const std::chrono::milliseconds timeout) {
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    while (!flag.load(std::memory_order_acquire) && std::chrono::steady_clock::now() < deadline) {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    return flag.load(std::memory_order_acquire);
}

}  // namespace

TEST(RuntimeLifecycleExecutorTest, ExecutesSubmittedTasksInOrder) {
    RuntimeLifecycleExecutor executor;
    std::mutex valuesLock;
    std::vector<int> values;

    for (int value = 1; value <= 3; ++value) {
        ASSERT_TRUE(executor.submit(
            [&, value] {
                const std::lock_guard lock(valuesLock);
                values.push_back(value);
            },
            std::chrono::seconds(5)));
    }

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    const std::lock_guard lock(valuesLock);
    EXPECT_EQ(values, (std::vector<int>{1, 2, 3}));
}

TEST(RuntimeLifecycleExecutorTest, DispatchesTasksThroughConfiguredDispatcher) {
    std::atomic<int> dispatches{0};
    std::atomic<int> executions{0};
    RuntimeLifecycleExecutor executor([&](RuntimeLifecycleExecutor::Task task) {
        dispatches.fetch_add(1, std::memory_order_acq_rel);
        task();
    });

    ASSERT_TRUE(executor.submit([&] { executions.fetch_add(1, std::memory_order_acq_rel); },
                                std::chrono::seconds(5)));

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(dispatches.load(std::memory_order_acquire), 1);
    EXPECT_EQ(executions.load(std::memory_order_acquire), 1);
}

TEST(RuntimeLifecycleExecutorTest, NeverRunsTwoLifecycleTasksConcurrently) {
    RuntimeLifecycleExecutor executor;
    std::atomic<int> active{0};
    std::atomic<int> maximum{0};

    for (int index = 0; index < 8; ++index) {
        ASSERT_TRUE(executor.submit(
            [&] {
                const auto current = active.fetch_add(1, std::memory_order_acq_rel) + 1;
                auto observed = maximum.load(std::memory_order_acquire);
                while (current > observed && !maximum.compare_exchange_weak(
                                                 observed, current, std::memory_order_acq_rel)) {
                }
                std::this_thread::sleep_for(std::chrono::milliseconds(2));
                active.fetch_sub(1, std::memory_order_acq_rel);
            },
            std::chrono::seconds(5)));
    }

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(maximum.load(std::memory_order_acquire), 1);
}

TEST(RuntimeLifecycleExecutorTest, StopRejectsNewLifecycleTasks) {
    RuntimeLifecycleExecutor executor;
    executor.requestStop();
    EXPECT_FALSE(executor.submit([] {}, std::chrono::seconds(5)));
    EXPECT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
}

TEST(RuntimeLifecycleExecutorTest, CoalescesLatestStateTaskByKey) {
    RuntimeLifecycleExecutor executor;
    std::atomic<bool> blockerStarted{false};
    std::atomic<bool> releaseBlocker{false};
    std::atomic<int> executions{0};
    std::atomic<int> value{0};

    ASSERT_TRUE(executor.submit(
        [&] {
            blockerStarted.store(true, std::memory_order_release);
            while (!releaseBlocker.load(std::memory_order_acquire)) std::this_thread::yield();
        },
        std::chrono::seconds(5)));
    if (!waitForFlag(blockerStarted, std::chrono::seconds(1))) {
        releaseBlocker.store(true, std::memory_order_release);
        FAIL() << "Lifecycle worker did not start the blocker task.";
    }

    ASSERT_EQ(executor.submitState(
                  "track/device/parameter/7",
                  [&] {
                      executions.fetch_add(1, std::memory_order_acq_rel);
                      value.store(1, std::memory_order_release);
                  },
                  std::chrono::seconds(5)),
              RuntimeLifecycleExecutor::StateSubmitResult::accepted);
    ASSERT_EQ(executor.submitState(
                  "track/device/parameter/7",
                  [&] {
                      executions.fetch_add(1, std::memory_order_acq_rel);
                      value.store(2, std::memory_order_release);
                  },
                  std::chrono::seconds(5)),
              RuntimeLifecycleExecutor::StateSubmitResult::coalesced);
    releaseBlocker.store(true, std::memory_order_release);

    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(executions.load(std::memory_order_acquire), 1);
    EXPECT_EQ(value.load(std::memory_order_acquire), 2);
}

TEST(RuntimeLifecycleExecutorTest, DropsOldestStateWhenCapacityIsExceeded) {
    RuntimeLifecycleExecutor executor;
    std::atomic<bool> blockerStarted{false};
    std::atomic<bool> releaseBlocker{false};
    std::atomic<int> newestValue{0};

    ASSERT_TRUE(executor.submit(
        [&] {
            blockerStarted.store(true, std::memory_order_release);
            while (!releaseBlocker.load(std::memory_order_acquire)) std::this_thread::yield();
        },
        std::chrono::seconds(5)));
    if (!waitForFlag(blockerStarted, std::chrono::seconds(1))) {
        releaseBlocker.store(true, std::memory_order_release);
        FAIL() << "Lifecycle worker did not start the blocker task.";
    }

    for (int index = 0; index < 256; ++index) {
        EXPECT_EQ(executor.submitState(
                      "state-" + std::to_string(index),
                      [&, index] { newestValue.store(index, std::memory_order_release); },
                      std::chrono::seconds(5)),
                  RuntimeLifecycleExecutor::StateSubmitResult::accepted);
    }
    EXPECT_EQ(executor.submitState(
                  "state-newest", [&] { newestValue.store(999, std::memory_order_release); },
                  std::chrono::seconds(5)),
              RuntimeLifecycleExecutor::StateSubmitResult::droppedCapacity);

    releaseBlocker.store(true, std::memory_order_release);
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(newestValue.load(std::memory_order_acquire), 999);
}

TEST(RuntimeLifecycleExecutorTest, FiresTimeoutHandlerWhenTaskExceedsItsTimeBound) {
    RuntimeLifecycleExecutor executor;
    std::atomic<int> timeouts{0};
    executor.setTimeoutHandler([&] { timeouts.fetch_add(1, std::memory_order_acq_rel); });
    std::atomic<bool> blockerStarted{false};
    std::atomic<bool> releaseBlocker{false};

    ASSERT_TRUE(executor.submit(
        [&] {
            blockerStarted.store(true, std::memory_order_release);
            while (!releaseBlocker.load(std::memory_order_acquire))
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
        },
        std::chrono::milliseconds(50)));
    if (!waitForFlag(blockerStarted, std::chrono::seconds(1))) {
        releaseBlocker.store(true, std::memory_order_release);
        FAIL() << "Lifecycle worker did not start the blocker task.";
    }

    std::this_thread::sleep_for(std::chrono::milliseconds(250));
    EXPECT_GE(timeouts.load(std::memory_order_acquire), 1);

    releaseBlocker.store(true, std::memory_order_release);
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(timeouts.load(std::memory_order_acquire), 1);
}

TEST(RuntimeLifecycleExecutorTest, DoesNotFireTimeoutHandlerWithinTimeBound) {
    RuntimeLifecycleExecutor executor;
    std::atomic<int> timeouts{0};
    executor.setTimeoutHandler([&] { timeouts.fetch_add(1, std::memory_order_acq_rel); });

    ASSERT_TRUE(executor.submit([] { std::this_thread::sleep_for(std::chrono::milliseconds(80)); },
                                std::chrono::seconds(1)));
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(2)));
    EXPECT_EQ(timeouts.load(std::memory_order_acquire), 0);
}

TEST(RuntimeLifecycleExecutorTest, ContinuesExecutingTasksAfterTimeoutFires) {
    RuntimeLifecycleExecutor executor;
    std::atomic<int> timeouts{0};
    executor.setTimeoutHandler([&] { timeouts.fetch_add(1, std::memory_order_acq_rel); });
    std::atomic<bool> blockerStarted{false};
    std::atomic<bool> releaseBlocker{false};
    std::atomic<int> laterRuns{0};

    ASSERT_TRUE(executor.submit(
        [&] {
            blockerStarted.store(true, std::memory_order_release);
            while (!releaseBlocker.load(std::memory_order_acquire))
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
        },
        std::chrono::milliseconds(50)));
    if (!waitForFlag(blockerStarted, std::chrono::seconds(1))) {
        releaseBlocker.store(true, std::memory_order_release);
        FAIL() << "Lifecycle worker did not start the blocker task.";
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(250));
    EXPECT_GE(timeouts.load(std::memory_order_acquire), 1);

    releaseBlocker.store(true, std::memory_order_release);
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    ASSERT_TRUE(executor.submit([&] { laterRuns.store(7, std::memory_order_release); },
                                std::chrono::seconds(5)));
    ASSERT_TRUE(executor.waitForIdle(std::chrono::seconds(1)));
    EXPECT_EQ(laterRuns.load(std::memory_order_acquire), 7);
    EXPECT_EQ(timeouts.load(std::memory_order_acquire), 1);
}

}  // namespace riffra
