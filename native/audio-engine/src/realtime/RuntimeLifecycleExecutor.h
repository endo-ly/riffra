#pragma once

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <deque>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>

namespace riffra {

/// Owns the serial execution boundary for every operation that can enter
/// third-party VST lifecycle code. The command reader only enqueues work; it
/// never joins a lifecycle worker or calls a plugin directly.
class RuntimeLifecycleExecutor final {
public:
    using Task = std::function<void()>;

    enum class StateSubmitResult {
        accepted,
        coalesced,
        droppedCapacity,
        stopping,
        invalid,
    };

    RuntimeLifecycleExecutor();
    ~RuntimeLifecycleExecutor();

    RuntimeLifecycleExecutor(const RuntimeLifecycleExecutor&) = delete;
    RuntimeLifecycleExecutor& operator=(const RuntimeLifecycleExecutor&) = delete;

    [[nodiscard]] bool submit(Task task);
    /// Enqueues a latest-value state event. Events with the same key replace
    /// one another, and a bounded state lane prevents parameter floods from
    /// delaying lifecycle work.
    [[nodiscard]] StateSubmitResult submitState(std::string key, Task task);
    [[nodiscard]] bool isBusy() const noexcept;
    [[nodiscard]] bool waitForIdle(std::chrono::milliseconds timeout) noexcept;
    void requestStop() noexcept;
    void join() noexcept;

private:
    void run();

    static constexpr std::size_t kStateTaskLimit = 256;

    mutable std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable idleChanged;
    std::deque<Task> lifecycleTasks;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, Task> stateTasks;
    std::thread worker;
    bool stopping = false;
    bool running = false;
};

}  // namespace riffra
