#pragma once

#include <cstddef>
#include <deque>
#include <string>
#include <unordered_map>
#include <utility>

namespace riffra {

/// Queues ordered control output separately from lossy telemetry output.
class OutputQueue final {
public:
    static constexpr std::size_t kTelemetryQueueLimit = 32;
    static constexpr std::size_t kStateQueueLimit = 256;

    /// Drops queued telemetry before adding a control response so the response
    /// becomes an ordering barrier for older runtime state.
    void enqueueControl(std::string line) {
        telemetryQueue.clear();
        controlQueue.push_back(std::move(line));
    }

    /// Adds telemetry unless the lossy queue is already at capacity.
    void enqueueTelemetry(std::string line) {
        if (telemetryQueue.size() >= kTelemetryQueueLimit) return;
        telemetryQueue.push_back(std::move(line));
    }

    /// Coalesces normal state notifications by key.
    void enqueueState(std::string key, std::string line) {
        if (const auto existing = stateQueue.find(key); existing != stateQueue.end()) {
            existing->second = std::move(line);
        } else if (stateQueue.size() < kStateQueueLimit) {
            stateOrder.push_back(key);
            stateQueue.emplace(std::move(key), std::move(line));
        }
    }

    /// Preserves the preceding state order and guarantees space for a final snapshot.
    void enqueueClosingState(std::string line) {
        for (const auto& key : stateOrder) {
            const auto event = stateQueue.find(key);
            if (event != stateQueue.end()) enqueueControl(std::move(event->second));
        }
        stateOrder.clear();
        stateQueue.clear();
        enqueueControl(std::move(line));
    }

    void discardStates() {
        stateOrder.clear();
        stateQueue.clear();
    }

    [[nodiscard]] bool hasControl() const noexcept { return !controlQueue.empty(); }
    [[nodiscard]] bool empty() const noexcept {
        return controlQueue.empty() && stateQueue.empty() && telemetryQueue.empty();
    }

    /// Removes the oldest control response. The caller must check `hasControl` first.
    std::string takeControl() {
        auto line = std::move(controlQueue.front());
        controlQueue.pop_front();
        return line;
    }

    /// Removes the oldest telemetry frame. Requires `!hasControl() && !empty()`.
    std::string takeTelemetry() {
        if (!stateQueue.empty()) {
            auto key = std::move(stateOrder.front());
            stateOrder.pop_front();
            auto event = stateQueue.find(key);
            auto line = std::move(event->second);
            stateQueue.erase(event);
            return line;
        }
        auto line = std::move(telemetryQueue.front());
        telemetryQueue.pop_front();
        return line;
    }

private:
    std::deque<std::string> controlQueue;
    std::deque<std::string> telemetryQueue;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, std::string> stateQueue;
};

}  // namespace riffra
