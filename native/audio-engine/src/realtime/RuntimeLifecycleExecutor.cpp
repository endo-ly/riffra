#include "RuntimeLifecycleExecutor.h"

#include <cstdlib>
#include <utility>

namespace riffra {

RuntimeLifecycleExecutor::RuntimeLifecycleExecutor()
    : worker([this] { run(); }) {}

RuntimeLifecycleExecutor::~RuntimeLifecycleExecutor() {
    requestStop();
    if (!waitForIdle(std::chrono::milliseconds(1500)))
        std::_Exit(0);
    join();
}

bool RuntimeLifecycleExecutor::submit(Task task) {
    if (!task)
        return false;
    {
        const std::lock_guard lock(mutex);
        if (stopping)
            return false;
        lifecycleTasks.push_back(std::move(task));
    }
    wake.notify_one();
    return true;
}

bool RuntimeLifecycleExecutor::submitState(std::string key, Task task) {
    if (!task || key.empty())
        return false;
    {
        const std::lock_guard lock(mutex);
        if (stopping)
            return false;
        if (const auto existing = stateTasks.find(key); existing != stateTasks.end()) {
            existing->second = std::move(task);
        } else {
            if (stateTasks.size() >= kStateTaskLimit)
                return false;
            stateOrder.push_back(key);
            stateTasks.emplace(std::move(key), std::move(task));
        }
    }
    wake.notify_one();
    return true;
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
}

void RuntimeLifecycleExecutor::run() {
    for (;;) {
        Task task;
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
                task = std::move(lifecycleTasks.front());
                lifecycleTasks.pop_front();
            } else {
                const auto key = std::move(stateOrder.front());
                stateOrder.pop_front();
                const auto event = stateTasks.find(key);
                if (event != stateTasks.end()) {
                    task = std::move(event->second);
                    stateTasks.erase(event);
                }
            }
            running = true;
        }
        try {
            task();
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
