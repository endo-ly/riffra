#pragma once

#include <cstddef>
#include <deque>
#include <string>
#include <utility>

namespace riffra {

/// Queues ordered control output separately from lossy telemetry output.
class OutputQueue final {
public:
    static constexpr std::size_t kTelemetryQueueLimit = 32;

    /// Drops queued telemetry before adding a control response so the response
    /// becomes an ordering barrier for older runtime state.
    [[nodiscard]] std::size_t enqueueControl(std::string line) {
        const auto dropped = telemetryQueue.size();
        telemetryQueue.clear();
        controlQueue.push_back(std::move(line));
        return dropped;
    }

    /// Adds telemetry unless the lossy queue is already at capacity.
    [[nodiscard]] bool enqueueTelemetry(std::string line) {
        if (telemetryQueue.size() >= kTelemetryQueueLimit) return false;
        telemetryQueue.push_back(std::move(line));
        return true;
    }

    [[nodiscard]] bool hasControl() const noexcept { return !controlQueue.empty(); }
    [[nodiscard]] bool hasTelemetry() const noexcept { return !telemetryQueue.empty(); }
    [[nodiscard]] bool empty() const noexcept {
        return controlQueue.empty() && telemetryQueue.empty();
    }

    /// Removes the oldest control response. The caller must check `hasControl` first.
    std::string takeControl() {
        auto line = std::move(controlQueue.front());
        controlQueue.pop_front();
        return line;
    }

    /// Removes the oldest telemetry frame. The caller must check `hasTelemetry` first.
    std::string takeTelemetry() {
        auto line = std::move(telemetryQueue.front());
        telemetryQueue.pop_front();
        return line;
    }

private:
    std::deque<std::string> controlQueue;
    std::deque<std::string> telemetryQueue;
};

}  // namespace riffra
