#include <JuceHeader.h>

#include "OfflineRenderer.h"

#include <cstdint>
#include <iostream>
#include <string>

namespace {

juce::var makeError(const juce::String& message) {
    auto* value = new juce::DynamicObject();
    value->setProperty("type", "error");
    value->setProperty("scope", "offlineRender");
    value->setProperty("message", message);
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

    const auto request = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
    if (!request.isObject()
        || request.getProperty("type", {}).toString() != "renderTimelineOffline"
        || static_cast<int>(request.getProperty("protocolVersion", 0)) != 1) {
        writeJson(makeError("Offline Render request is invalid."));
        return 1;
    }
    const auto destination = request.getProperty("destination", {}).toString();
    const auto startTick =
        static_cast<juce::int64>(request.getProperty("startTick", -1));
    const auto endTick =
        static_cast<juce::int64>(request.getProperty("endTick", -1));
    if (destination.isEmpty() || startTick < 0 || endTick <= startTick) {
        writeJson(makeError("Offline Render destination or range is invalid."));
        return 1;
    }

    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    riffra::OfflineRenderer renderer;
    riffra::OfflineRenderer::Result result;
    juce::String error;
    if (!renderer.render(
            request.getProperty("snapshot", {}),
            formats,
            juce::File(destination),
            static_cast<std::uint64_t>(startTick),
            static_cast<std::uint64_t>(endTick),
            static_cast<double>(request.getProperty("sampleRate", 0.0)),
            static_cast<int>(request.getProperty("blockSize", 0)),
            static_cast<float>(request.getProperty("masterGainDb", 0.0)),
            static_cast<bool>(request.getProperty("normalize", false)),
            result,
            error)) {
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

} // namespace

#if JUCE_WINDOWS
int wmain() {
    return runRenderWorker();
}
#else
int main() {
    return runRenderWorker();
}
#endif
