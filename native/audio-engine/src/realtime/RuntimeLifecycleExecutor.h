#pragma once

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <deque>
#include <functional>
#include <mutex>
#include <string>
#include <thread>

namespace riffra {

/// Owns the serial execution boundary for every operation that can enter
/// third-party VST lifecycle code. The command reader only enqueues work; it
/// never joins a lifecycle worker or calls a plugin directly.
class RuntimeLifecycleExecutor final {
public:
    using Task = std::function<void()>;

    RuntimeLifecycleExecutor();
    ~RuntimeLifecycleExecutor();

    RuntimeLifecycleExecutor(const RuntimeLifecycleExecutor&) = delete;
    RuntimeLifecycleExecutor& operator=(const RuntimeLifecycleExecutor&) = delete;

    [[nodiscard]] bool submit(Task task);
    [[nodiscard]] bool isBusy() const noexcept;
    [[nodiscard]] bool waitForIdle(std::chrono::milliseconds timeout) noexcept;
    void requestStop() noexcept;
    void join() noexcept;

private:
    void run();

    mutable std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable idleChanged;
    std::deque<Task> tasks;
    std::thread worker;
    bool stopping = false;
    bool running = false;
};

}  // namespace riffra
