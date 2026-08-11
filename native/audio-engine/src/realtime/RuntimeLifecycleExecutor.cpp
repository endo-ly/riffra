#include "RuntimeLifecycleExecutor.h"

#include <cstdlib>
#include <utility>

namespace riffra {

RuntimeLifecycleExecutor::RuntimeLifecycleExecutor(TaskDispatcher dispatcher)
    : taskDispatcher(std::move(dispatcher)),
      timeoutHandler([] { std::_Exit(124); }) {
    // Do not start threads from the member-initializer list. At that point
    // later members (including the watchdog handler and lifecycle flags) have
    // not necessarily been initialized, and the thread would observe a
    // partially constructed object.
    worker = std::thread([this] { run(); });
    watchdog = std::thread([this] { watch(); });
}

RuntimeLifecycleExecutor::~RuntimeLifecycleExecutor() {
    requestStop();
    if (!waitForIdle(std::chrono::milliseconds(1500)))
        std::_Exit(125);
    join();
}

bool RuntimeLifecycleExecutor::submit(
    Task task,
    std::chrono::milliseconds timeout) {
    if (!task || timeout <= std::chrono::milliseconds::zero())
        return false;
    {
        const std::lock_guard lock(mutex);
        if (stopping)
            return false;
        lifecycleTasks.push_back(TimedTask { std::move(task), timeout });
    }
    wake.notify_one();
    return true;
}

RuntimeLifecycleExecutor::StateSubmitResult RuntimeLifecycleExecutor::submitState(
    std::string key,
    Task task,
    std::chrono::milliseconds timeout) {
    if (!task || key.empty() || timeout <= std::chrono::milliseconds::zero())
        return StateSubmitResult::invalid;
    {
        const std::lock_guard lock(mutex);
        if (stopping)
            return StateSubmitResult::stopping;
        if (const auto existing = stateTasks.find(key); existing != stateTasks.end()) {
            existing->second = TimedTask { std::move(task), timeout };
            return StateSubmitResult::coalesced;
        } else {
            StateSubmitResult result = StateSubmitResult::accepted;
            if (stateTasks.size() >= kStateTaskLimit) {
                while (!stateOrder.empty()) {
                    const auto oldestKey = std::move(stateOrder.front());
                    stateOrder.pop_front();
                    if (stateTasks.erase(oldestKey) != 0)
                        break;
                }
                result = StateSubmitResult::droppedCapacity;
            }
            stateOrder.push_back(key);
            stateTasks.emplace(std::move(key), TimedTask { std::move(task), timeout });
            wake.notify_one();
            return result;
        }
    }
}

void RuntimeLifecycleExecutor::setTimeoutHandler(TimeoutHandler handler) noexcept {
    if (!handler)
        return;
    const std::lock_guard lock(mutex);
    timeoutHandler = std::move(handler);
}

bool RuntimeLifecycleExecutor::isBusy() const noexcept {
    const std::lock_guard lock(mutex);
    return running || !lifecycleTasks.empty() || !stateTasks.empty();
}

bool RuntimeLifecycleExecutor::waitForIdle(const std::chrono::milliseconds timeout) noexcept {
    std::unique_lock lock(mutex);
    return idleChanged.wait_for(lock, timeout, [this] {
        return !running && lifecycleTasks.empty() && stateTasks.empty();
    });
}

void RuntimeLifecycleExecutor::requestStop() noexcept {
    {
        const std::lock_guard lock(mutex);
        stopping = true;
        lifecycleTasks.clear();
        stateOrder.clear();
        stateTasks.clear();
    }
    wake.notify_all();
    idleChanged.notify_all();
}

void RuntimeLifecycleExecutor::join() noexcept {
    if (worker.joinable())
        worker.join();
    if (watchdog.joinable())
        watchdog.join();
}

/// The watchdog thread is the only defense against a third-party VST that
/// blocks forever inside a lifecycle task. It samples the running task's
/// elapsed time; on timeout it invokes the installed handler exactly once.
/// The wedged worker thread is left to finish on its own — a blocked plugin
/// cannot be interrupted safely — and the process is expected to terminate
/// (or the supervisor restarts it) right after the handler runs.
void RuntimeLifecycleExecutor::watch() {
    for (;;) {
        std::this_thread::sleep_for(kWatchdogGranularity);
        bool timedOut = false;
        {
            std::unique_lock lock(mutex);
            if (stopping)
                return;
            if (!running || currentTaskTimedOut)
                continue;
            if (std::chrono::steady_clock::now() - currentTaskStarted > currentTaskTimeout) {
                timedOut = true;
                currentTaskTimedOut = true;
            }
        }
        if (timedOut) {
            TimeoutHandler handler;
            {
                const std::lock_guard lock(mutex);
                handler = timeoutHandler;
            }
            handler();
            return;
        }
    }
}

void RuntimeLifecycleExecutor::run() {
    for (;;) {
        TimedTask timedTask;
        {
            std::unique_lock lock(mutex);
            wake.wait(lock, [this] {
                return stopping || !lifecycleTasks.empty() || !stateTasks.empty();
            });
            if (stopping && lifecycleTasks.empty() && stateTasks.empty()) {
                running = false;
                idleChanged.notify_all();
                return;
            }
            if (!lifecycleTasks.empty()) {
                timedTask = std::move(lifecycleTasks.front());
                lifecycleTasks.pop_front();
            } else {
                const auto key = std::move(stateOrder.front());
                stateOrder.pop_front();
                const auto event = stateTasks.find(key);
                if (event != stateTasks.end()) {
                    timedTask = std::move(event->second);
                    stateTasks.erase(event);
                }
            }
            running = true;
            currentTaskTimedOut = false;
            currentTaskStarted = std::chrono::steady_clock::now();
            currentTaskTimeout = timedTask.timeout;
        }
        try {
            if (taskDispatcher)
                taskDispatcher(std::move(timedTask.task));
            else
                timedTask.task();
        } catch (...) {
            // Lifecycle tasks report expected failures through their command
            // response. A defensive boundary keeps one unexpected vendor
            // exception from terminating the executor thread itself.
        }
        {
            const std::lock_guard lock(mutex);
            running = false;
        }
        idleChanged.notify_all();
    }
}

}  // namespace riffra
