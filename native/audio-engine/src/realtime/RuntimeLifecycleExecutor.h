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
///
/// A stuck third-party VST (constructor, destructor, editor, or parameter
/// path) must never wedge the engine permanently. Each task therefore runs
/// under a watchdog: when the running task exceeds its deadline the
/// [`RuntimeLifecycleExecutor::setTimeoutHandler`] callback fires so the owner
/// can terminate the process; the Rust supervisor treats the lost transport
/// as a restart condition and recovers in emergency-mute state. The watchdog
/// is deliberately coarse (third-party code cannot be interrupted safely);
/// process-level isolation is the recovery boundary.
class RuntimeLifecycleExecutor final {
public:
    using Task = std::function<void()>;
    using TimeoutHandler = std::function<void()>;

    enum class StateSubmitResult {
        accepted,
        coalesced,
        droppedCapacity,
        stopping,
        invalid,
    };

    /// The watchdog resolution (how often the running task is re-checked).
    static constexpr std::chrono::milliseconds kWatchdogGranularity { 25 };

    RuntimeLifecycleExecutor();
    ~RuntimeLifecycleExecutor();

    RuntimeLifecycleExecutor(const RuntimeLifecycleExecutor&) = delete;
    RuntimeLifecycleExecutor& operator=(const RuntimeLifecycleExecutor&) = delete;

    /// Enqueues a lifecycle task that must finish within `timeout` of starting
    /// to execute. Exceeding the timeout invokes the timeout handler once.
    [[nodiscard]] bool submit(Task task, std::chrono::milliseconds timeout);
    /// Enqueues a latest-value state event. Events with the same key replace
    /// one another, and a bounded state lane prevents parameter floods from
    /// delaying lifecycle work. State events are time-bounded like lifecycle
    /// tasks.
    [[nodiscard]] StateSubmitResult submitState(
        std::string key,
        Task task,
        std::chrono::milliseconds timeout);
    /// Installs the handler invoked when a running task exceeds its timeout.
    /// The default handler terminates the process. The handler runs on the
    /// watchdog thread and must never block.
    void setTimeoutHandler(TimeoutHandler handler) noexcept;
    [[nodiscard]] bool isBusy() const noexcept;
    [[nodiscard]] bool waitForIdle(std::chrono::milliseconds timeout) noexcept;
    void requestStop() noexcept;
    void join() noexcept;

private:
    struct TimedTask {
        Task task;
        std::chrono::milliseconds timeout { 0 };
    };

    void run();
    void watch();

    static constexpr std::size_t kStateTaskLimit = 256;

    mutable std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable idleChanged;
    std::deque<TimedTask> lifecycleTasks;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, TimedTask> stateTasks;
    TimeoutHandler timeoutHandler;
    bool stopping = false;
    bool running = false;
    bool currentTaskTimedOut = false;
    std::chrono::steady_clock::time_point currentTaskStarted {};
    std::chrono::milliseconds currentTaskTimeout { 0 };
    // Threads are declared after every piece of state they can observe and
    // are started in the constructor body, once the complete object exists.
    std::thread worker;
    std::thread watchdog;
};

}  // namespace riffra
