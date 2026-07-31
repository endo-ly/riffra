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
        tasks.push_back(std::move(task));
    }
    wake.notify_one();
    return true;
}

bool RuntimeLifecycleExecutor::isBusy() const noexcept {
    const std::lock_guard lock(mutex);
    return running || !tasks.empty();
}

bool RuntimeLifecycleExecutor::waitForIdle(const std::chrono::milliseconds timeout) noexcept {
    std::unique_lock lock(mutex);
    return idleChanged.wait_for(lock, timeout, [this] {
        return !running && tasks.empty();
    });
}

void RuntimeLifecycleExecutor::requestStop() noexcept {
    {
        const std::lock_guard lock(mutex);
        stopping = true;
        tasks.clear();
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
            wake.wait(lock, [this] { return stopping || !tasks.empty(); });
            if (stopping && tasks.empty()) {
                running = false;
                idleChanged.notify_all();
                return;
            }
            task = std::move(tasks.front());
            tasks.pop_front();
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
