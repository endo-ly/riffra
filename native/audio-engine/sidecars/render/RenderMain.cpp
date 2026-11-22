#include <JuceHeader.h>

#include <cstdint>
#include <iostream>
#include <string>
#include <thread>

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <poll.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

#include "contract/ContractReader.h"
#include "contract/ExecutionGraphDecoder.h"
#include "contract/SidecarMessages.h"
#include "protocol/ProtocolChannel.h"
#include "render/OfflineRenderer.h"

namespace {

constexpr auto kRenderOperation = "renderTimelineOffline";

/// True when the host closed the request pipe. The host holds it open for the whole render and
/// closes it to abandon the render, either because the job was cancelled or because the host
/// exited.
bool requestPipeClosed() noexcept {
#if JUCE_WINDOWS
    const auto input = GetStdHandle(STD_INPUT_HANDLE);
    if (GetFileType(input) != FILE_TYPE_PIPE) return false;
    return PeekNamedPipe(input, nullptr, 0, nullptr, nullptr, nullptr) == 0;
#else
    // poll(2) reports readiness for regular files too, and standard input is often redirected
    // from a file; only a pipe or socket reports its peer's close this way.
    struct stat input{};
    if (::fstat(STDIN_FILENO, &input) != 0) return false;
    if (!S_ISFIFO(input.st_mode) && !S_ISSOCK(input.st_mode)) return false;
    pollfd descriptor{STDIN_FILENO, POLLIN, 0};
    return ::poll(&descriptor, 1, 0) > 0;
#endif
}

/// Watches the request pipe on the message thread while the render runs. Cancelling stops the
/// render cooperatively, so its partial outputs are removed before the process exits.
class RequestPipeWatcher final : private juce::Timer {
public:
    explicit RequestPipeWatcher(riffra::OfflineRenderer& rendererToCancel)
        : renderer(rendererToCancel) {
        startTimer(25);
    }

    ~RequestPipeWatcher() override { stopTimer(); }

    RequestPipeWatcher(const RequestPipeWatcher&) = delete;
    RequestPipeWatcher& operator=(const RequestPipeWatcher&) = delete;

private:
    void timerCallback() override {
        if (!requestPipeClosed()) return;
        stopTimer();
        renderer.cancel();
    }

    riffra::OfflineRenderer& renderer;
};

void writeLine(const juce::var& value) {
    riffra::writeProtocolLine(juce::JSON::toString(value, true).toStdString());
}

int reject(const juce::String& kind, const juce::String& message) {
    writeLine(riffra::encodeOfflineRenderError({kind, message, kRenderOperation, {}}));
    return 1;
}

int runRenderWorker() {
    if (!riffra::isolateProtocolChannel())
        return reject("renderRejected",
                      "Could not isolate the protocol channel on standard output.");
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    std::string line;
    if (!std::getline(std::cin, line))
        return reject("renderContract", "Expected one Offline Render request on standard input.");

    const auto envelope = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
    juce::String type;
    std::uint32_t protocolVersion = 0;
    juce::var payload;
    juce::String contractError;
    riffra::ContractReader envelopeReader(envelope, "", {"type", "protocolVersion", "request"},
                                          contractError);
    if (!envelopeReader.string("type", type) ||
        !envelopeReader.unsigned32("protocolVersion", protocolVersion) ||
        !envelopeReader.object("request", payload) || !envelopeReader.finish())
        return reject("renderContract", contractError);
    if (type != kRenderOperation) return reject("renderContract", "Unknown request type: " + type);
    if (protocolVersion != riffra::kSidecarProtocolVersion)
        return reject("protocol",
                      "Unsupported protocol version " + juce::String(protocolVersion) + ".");
    riffra::OfflineRenderRequestSpec renderRequest;
    if (!riffra::decodeOfflineRenderRequest(payload, renderRequest, contractError))
        return reject("renderContract", contractError);

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    juce::String error;
    const auto renderer = riffra::OfflineRenderer::prepare(renderRequest, formats, error);
    if (renderer == nullptr) return reject("renderRejected", error);

    riffra::OfflineRenderer::Result result;
    auto rendered = false;
    RequestPipeWatcher watcher(*renderer);
    std::thread renderThread([&] {
        rendered = renderer->render(formats, result, error);
        juce::MessageManager::getInstance()->stopDispatchLoop();
    });
    juce::MessageManager::getInstance()->runDispatchLoop();
    renderThread.join();
    if (!rendered) return reject("renderRejected", error);

    writeLine(riffra::encodeOfflineRenderComplete(
        {result.frames, static_cast<std::uint32_t>(result.sampleRate), result.automationWarnings}));
    return 0;
}

}  // namespace

#if JUCE_WINDOWS
int wmain() { return runRenderWorker(); }
#else
int main() { return runRenderWorker(); }
#endif
