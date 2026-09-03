#include "AudioProtocol.h"

#include <array>
#include <atomic>
#include <cmath>
#include <condition_variable>
#include <deque>
#include <iostream>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <utility>

#include "FaultInjection.h"

namespace riffra {
namespace {

thread_local juce::String currentRequestIdValue;

class OutputWriter final {
public:
    OutputWriter() = default;

    ~OutputWriter() { stop(); }

    void enqueue(std::string line, const OutputKind kind) {
        if (kind == OutputKind::control) FaultInjection::stdoutFlood();
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (kind == OutputKind::telemetry && telemetryQueue.size() >= kTelemetryQueueLimit) {
                droppedTelemetry.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            if (kind == OutputKind::control)
                controlQueue.push_back(std::move(line));
            else
                telemetryQueue.push_back(std::move(line));
        }
        wake.notify_one();
    }

    void enqueueState(std::string key, std::string line) {
        if (key.empty()) {
            enqueue(std::move(line), OutputKind::telemetry);
            return;
        }
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (const auto existing = stateQueue.find(key); existing != stateQueue.end()) {
                existing->second = std::move(line);
            } else {
                if (stateQueue.size() >= kStateQueueLimit) {
                    droppedState.fetch_add(1, std::memory_order_relaxed);
                    return;
                }
                stateOrder.push_back(key);
                stateQueue.emplace(std::move(key), std::move(line));
            }
        }
        wake.notify_one();
    }

    [[nodiscard]] std::uint64_t droppedTelemetryCount() const noexcept {
        return droppedTelemetry.load(std::memory_order_acquire);
    }

    [[nodiscard]] std::uint64_t droppedStateCount() const noexcept {
        return droppedState.load(std::memory_order_acquire);
    }

private:
    static constexpr std::size_t kTelemetryQueueLimit = 32;
    static constexpr std::size_t kStateQueueLimit = 256;

    void ensureStarted() {
        if (writer.joinable()) return;
        writer = std::thread([this] { run(); });
    }

    void run() {
        for (;;) {
            std::string line;
            {
                std::unique_lock lock(mutex);
                wake.wait(lock, [this] {
                    return stopping || !controlQueue.empty() || !stateQueue.empty() ||
                           !telemetryQueue.empty();
                });
                if (stopping && controlQueue.empty() && stateQueue.empty() &&
                    telemetryQueue.empty())
                    return;
                if (!controlQueue.empty()) {
                    line = std::move(controlQueue.front());
                    controlQueue.pop_front();
                } else if (!stateQueue.empty()) {
                    const auto key = std::move(stateOrder.front());
                    stateOrder.pop_front();
                    const auto event = stateQueue.find(key);
                    if (event != stateQueue.end()) {
                        line = std::move(event->second);
                        stateQueue.erase(event);
                    }
                } else {
                    line = std::move(telemetryQueue.front());
                    telemetryQueue.pop_front();
                }
            }
            std::cout << line << '\n' << std::flush;
        }
    }

    void stop() {
        {
            const std::lock_guard lock(mutex);
            stopping = true;
            stateOrder.clear();
            stateQueue.clear();
        }
        wake.notify_one();
        if (writer.joinable()) writer.join();
    }

    mutable std::mutex mutex;
    std::condition_variable wake;
    std::deque<std::string> controlQueue;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, std::string> stateQueue;
    std::deque<std::string> telemetryQueue;
    std::thread writer;
    std::atomic<std::uint64_t> droppedTelemetry{0};
    std::atomic<std::uint64_t> droppedState{0};
    bool stopping = false;
};

OutputWriter outputWriter;

}  // namespace

void clearCurrentRequestId() noexcept { currentRequestIdValue.clear(); }

void setCurrentRequestId(const juce::String& requestId) { currentRequestIdValue = requestId; }

juce::String currentRequestId() { return currentRequestIdValue; }

std::uint64_t droppedTelemetryCount() noexcept { return outputWriter.droppedTelemetryCount(); }

std::uint64_t droppedStateCount() noexcept { return outputWriter.droppedStateCount(); }

juce::var makeError(const juce::String& scope, const juce::String& message) {
    auto* object = new juce::DynamicObject();
    object->setProperty("type", "error");
    object->setProperty("scope", scope);
    object->setProperty("message", message);
    object->setProperty("dataSafe", true);
    return juce::var(object);
}

bool parseMidiBytes(const juce::var& value, juce::MidiMessage& message, juce::String& error) {
    if (!value.isArray()) {
        error = "A bytes array of MIDI data is required.";
        return false;
    }
    const auto bytesArray = *value.getArray();
    if (bytesArray.isEmpty() || bytesArray.size() > 3) {
        error = "MIDI bytes must contain between 1 and 3 bytes.";
        return false;
    }
    std::array<std::uint8_t, 3> bytes{};
    for (int index = 0; index < bytesArray.size(); ++index) {
        const auto& valueAtIndex = bytesArray[index];
        if (!valueAtIndex.isInt() && !valueAtIndex.isInt64() && !valueAtIndex.isDouble()) {
            error = "MIDI bytes must be integer values.";
            return false;
        }
        const auto numeric = static_cast<double>(valueAtIndex);
        if (!std::isfinite(numeric) || std::floor(numeric) != numeric || numeric < 0.0 ||
            numeric > 255.0) {
            error = "MIDI bytes must be integers from 0 through 255.";
            return false;
        }
        bytes[static_cast<std::size_t>(index)] = static_cast<std::uint8_t>(numeric);
    }
    if ((bytes[0] & 0x80u) == 0u) {
        error = "The first MIDI byte must be a status byte.";
        return false;
    }
    for (int index = 1; index < bytesArray.size(); ++index) {
        if (bytes[static_cast<std::size_t>(index)] > 0x7fu) {
            error = "MIDI data bytes must be below 128.";
            return false;
        }
    }
    switch (bytesArray.size()) {
        case 1:
            message = juce::MidiMessage(bytes[0]);
            break;
        case 2:
            message = juce::MidiMessage(bytes[0], bytes[1]);
            break;
        default:
            message = juce::MidiMessage(bytes[0], bytes[1], bytes[2]);
            break;
    }
    return true;
}

void writeJson(const juce::var& value, const juce::String& requestId, const OutputKind kind,
               std::string stateKey) {
    auto response = value;
    const auto effectiveRequestId = requestId.isNotEmpty() ? requestId : currentRequestIdValue;
    if (effectiveRequestId.isNotEmpty())
        if (auto* object = response.getDynamicObject())
            object->setProperty("requestId", effectiveRequestId.getLargeIntValue());
    auto line = juce::JSON::toString(response, true).toStdString();
    if (kind == OutputKind::state)
        outputWriter.enqueueState(std::move(stateKey), std::move(line));
    else
        outputWriter.enqueue(std::move(line), kind);
}

}  // namespace riffra
