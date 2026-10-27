#include "AudioProtocol.h"

#include <condition_variable>
#include <mutex>
#include <string>
#include <thread>
#include <utility>

#include "OutputQueue.h"
#include "plugins/FaultInjection.h"
#include "protocol/ProtocolChannel.h"

namespace riffra {
namespace {

enum class OutputKind { control, telemetry };

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
            outputQueue.enqueueState(std::move(key), std::move(line));
        }
        wake.notify_one();
    }

    void enqueueClosingState(std::string line) {
        {
            const std::lock_guard lock(mutex);
            ensureStarted();
            outputQueue.enqueueClosingState(std::move(line));
        }
        wake.notify_one();
    }

private:
    void ensureStarted() {
        if (writer.joinable()) return;
        writer = std::thread([this] { run(); });
    }

    void run() {
        for (;;) {
            std::string line;
            {
                std::unique_lock lock(mutex);
                wake.wait(lock, [this] { return stopping || !outputQueue.empty(); });
                if (stopping && outputQueue.empty()) return;
                if (outputQueue.hasControl()) {
                    line = outputQueue.takeControl();
                } else {
                    line = outputQueue.takeTelemetry();
                }
            }
            writeProtocolLine(line);
        }
    }

    void stop() {
        {
            const std::lock_guard lock(mutex);
            stopping = true;
            outputQueue.discardStates();
        }
        wake.notify_one();
        if (writer.joinable()) writer.join();
    }

    std::mutex mutex;
    std::condition_variable wake;
    OutputQueue outputQueue;
    std::thread writer;
    bool stopping = false;
};

OutputWriter outputWriter;

std::string line(const juce::var& value) { return juce::JSON::toString(value, true).toStdString(); }

std::string deviceKey(const juce::String& projectId, const juce::String& deviceId) {
    return (projectId + ":" + deviceId).toStdString();
}

}  // namespace

void writeControlEnvelope(const juce::var& envelope) {
    outputWriter.enqueue(line(envelope), OutputKind::control);
}

void writeClosingPluginState(const PluginStateChangedSpec& event) {
    outputWriter.enqueueClosingState(line(encodeEvent(event)));
}

void writeEvent(const SidecarEventSpec& event) {
    auto encoded = line(encodeEvent(event));
    if (const auto* changed = std::get_if<PluginStateChangedSpec>(&event)) {
        outputWriter.enqueueState(
            "plugin-state:" + deviceKey(changed->projectId, changed->deviceId), std::move(encoded));
    } else if (const auto* parameter = std::get_if<PluginParameterChangedSpec>(&event)) {
        outputWriter.enqueueState(
            "plugin-parameter:" + deviceKey(parameter->projectId, parameter->deviceId) + ":" +
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
