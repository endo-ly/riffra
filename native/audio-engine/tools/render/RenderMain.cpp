#include <JuceHeader.h>

#include <cstdint>
#include <iostream>
#include <string>
#include <thread>

#include "contract/ContractReader.h"
#include "contract/ExecutionGraphDecoder.h"
#include "contract/SidecarMessages.h"
#include "protocol/ProtocolChannel.h"
#include "render/OfflineRenderer.h"

namespace {

constexpr auto kRenderOperation = "renderTimelineOffline";

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
    std::thread renderThread([&] {
        rendered = renderer->render(formats, result, error);
        juce::MessageManager::getInstance()->stopDispatchLoop();
    });
    juce::MessageManager::getInstance()->runDispatchLoop();
    renderThread.join();
    if (!rendered) return reject("renderRejected", error);

    writeLine(riffra::encodeOfflineRenderComplete(
        {result.frames, static_cast<std::uint32_t>(result.sampleRate)}));
    return 0;
}

}  // namespace

#if JUCE_WINDOWS
int wmain() { return runRenderWorker(); }
#else
int main() { return runRenderWorker(); }
#endif
