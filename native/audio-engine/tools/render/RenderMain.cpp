#include <JuceHeader.h>

#include <cstdint>
#include <iostream>
#include <string>

#include "contract/ContractReader.h"
#include "contract/ExecutionGraphDecoder.h"
#include "render/OfflineRenderer.h"

namespace {

juce::var makeError(const juce::String& message, const juce::String& kind = "renderRejected",
                    const juce::String& operation = "renderTimelineOffline") {
    auto* value = new juce::DynamicObject();
    value->setProperty("type", "error");
    value->setProperty("kind", kind);
    value->setProperty("message", message);
    value->setProperty("operation", operation);
    return juce::var(value);
}

void writeJson(const juce::var& value) {
    std::cout << juce::JSON::toString(value, true) << std::endl;
}

int runRenderWorker() {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    std::string line;
    if (!std::getline(std::cin, line)) {
        writeJson(makeError("Expected one Offline Render request on standard input."));
        return 1;
    }

    const auto envelope = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
    juce::String type;
    std::uint32_t protocolVersion = 0;
    juce::var payload;
    juce::String contractError;
    riffra::ContractReader envelopeReader(envelope, "", {"type", "protocolVersion", "request"},
                                          contractError);
    if (!envelopeReader.string("type", type) ||
        !envelopeReader.unsigned32("protocolVersion", protocolVersion) ||
        !envelopeReader.object("request", payload) || !envelopeReader.finish()) {
        writeJson(makeError(contractError, "renderContract", "renderTimelineOffline"));
        return 1;
    }
    if (type != "renderTimelineOffline" || protocolVersion != 2) {
        writeJson(makeError("Offline Render request is invalid."));
        return 1;
    }
    riffra::OfflineRenderRequestSpec renderRequest;
    if (!riffra::decodeOfflineRenderRequest(payload, renderRequest, contractError)) {
        writeJson(makeError(contractError, "renderContract", "renderTimelineOffline"));
        return 1;
    }

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    riffra::OfflineRenderer renderer;
    riffra::OfflineRenderer::Result result;
    juce::String error;
    if (!renderer.render(renderRequest, formats, result, error)) {
        writeJson(makeError(error));
        return 1;
    }

    auto* response = new juce::DynamicObject();
    response->setProperty("type", "offlineRenderComplete");
    response->setProperty("frames", static_cast<juce::int64>(result.frames));
    response->setProperty("sampleRate", result.sampleRate);
    writeJson(juce::var(response));
    return 0;
}

}  // namespace

#if JUCE_WINDOWS
int wmain() { return runRenderWorker(); }
#else
int main() { return runRenderWorker(); }
#endif
