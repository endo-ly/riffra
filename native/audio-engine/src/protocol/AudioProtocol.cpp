#include "AudioProtocol.h"

#include <condition_variable>
#include <deque>
#include <iostream>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>

#include "OutputQueue.h"
#include "plugins/FaultInjection.h"

namespace riffra {
namespace {

enum class OutputKind { control, state, telemetry };

class OutputWriter final {
public:
    OutputWriter() = default;

    ~OutputWriter() { stop(); }

    void enqueue(std::string line, const OutputKind kind) {
        if (kind == OutputKind::control) FaultInjection::stdoutFlood();
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (kind == OutputKind::control) {
                // A control message is an ordering barrier for lossy telemetry. Any telemetry
                // still queued here describes an earlier state and must not overtake it.
                outputQueue.enqueueControl(std::move(line));
            } else {
                outputQueue.enqueueTelemetry(std::move(line));
            }
        }
        wake.notify_one();
    }

    void enqueueState(std::string key, std::string line) {
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            if (const auto existing = stateQueue.find(key); existing != stateQueue.end()) {
                existing->second = std::move(line);
            } else {
                if (stateQueue.size() >= kStateQueueLimit) return;
                stateOrder.push_back(key);
                stateQueue.emplace(std::move(key), std::move(line));
            }
        }
        wake.notify_one();
    }

private:
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
                    return stopping || !outputQueue.empty() || !stateQueue.empty();
                });
                if (stopping && outputQueue.empty() && stateQueue.empty()) return;
                if (outputQueue.hasControl()) {
                    line = outputQueue.takeControl();
                } else if (!stateQueue.empty()) {
                    const auto key = std::move(stateOrder.front());
                    stateOrder.pop_front();
                    const auto event = stateQueue.find(key);
                    if (event != stateQueue.end()) {
                        line = std::move(event->second);
                        stateQueue.erase(event);
                    }
                } else {
                    line = outputQueue.takeTelemetry();
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

    std::mutex mutex;
    std::condition_variable wake;
    std::deque<std::string> stateOrder;
    std::unordered_map<std::string, std::string> stateQueue;
    OutputQueue outputQueue;
    std::thread writer;
    bool stopping = false;
};

OutputWriter outputWriter;

std::string line(const juce::var& value) { return juce::JSON::toString(value, true).toStdString(); }

std::string deviceKey(const juce::String& trackId, const juce::String& deviceId) {
    return (trackId + ":" + deviceId).toStdString();
}

}  // namespace

void writeControlEnvelope(const juce::var& envelope) {
    outputWriter.enqueue(line(envelope), OutputKind::control);
}

void writeEvent(const SidecarEventSpec& event) {
    auto encoded = line(encodeEvent(event));
    if (const auto* changed = std::get_if<TrackPluginStateChangedSpec>(&event)) {
        outputWriter.enqueueState("track-state:" + deviceKey(changed->trackId, changed->deviceId),
                                  std::move(encoded));
    } else if (const auto* parameter = std::get_if<TrackPluginParameterChangedSpec>(&event)) {
        outputWriter.enqueueState(
            "track-parameter:" + deviceKey(parameter->trackId, parameter->deviceId) + ":" +
                std::to_string(parameter->parameterIndex),
            std::move(encoded));
    } else if (std::holds_alternative<AudioStatusSpec>(event)) {
        outputWriter.enqueueState("audio-status", std::move(encoded));
    } else if (std::holds_alternative<AudioMetersSpec>(event) ||
               std::holds_alternative<TransportStatusSpec>(event)) {
        outputWriter.enqueue(std::move(encoded), OutputKind::telemetry);
    } else {
        outputWriter.enqueue(std::move(encoded), OutputKind::control);
    }
}

}  // namespace riffra
