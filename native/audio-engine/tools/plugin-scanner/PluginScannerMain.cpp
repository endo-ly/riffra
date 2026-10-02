#include <JuceHeader.h>

#include <optional>

#include "contract/SidecarMessages.h"
#include "plugins/PluginRack.h"
#include "protocol/ProtocolChannel.h"

namespace {

void writeJson(const juce::var& value) {
    riffra::writeProtocolLine(juce::JSON::toString(value, true).toStdString());
}

juce::var makeError(const juce::String& path, const juce::String& message) {
    return riffra::encodePluginScanError({path, message});
}

juce::var makeLoadTestResult(const juce::String& path, const bool success,
                             const juce::String& message, const double durationMs) {
    return riffra::encodePluginLoadTestResult({path, success, message, durationMs});
}

std::optional<juce::String> validateInstanceCreation(const juce::String& path) {
    riffra::PluginRack rack;
    if (const auto loadError =
            rack.load(path, 44100.0, 512, riffra::PluginProcessingMode::realtime))
        return loadError->message;
    return std::nullopt;
}

int scan(const juce::String& path) {
    const auto started = juce::Time::getMillisecondCounterHiRes();
    if (!juce::File(path).exists()) {
        writeJson(makeError(path, "VST3 bundle or file does not exist."));
        return 2;
    }

    juce::VST3PluginFormat format;
    juce::OwnedArray<juce::PluginDescription> descriptions;
    format.findAllTypesForFile(descriptions, path);
    if (descriptions.isEmpty()) {
        writeJson(makeError(path, "No VST3 component could be described."));
        return 3;
    }

    riffra::PluginScanResultSpec result;
    result.path = path;
    for (const auto* description : descriptions)
        if (description != nullptr) result.plugins.push_back(*description);
    const auto loadTestStarted = juce::Time::getMillisecondCounterHiRes();
    const auto loadError = validateInstanceCreation(path);
    result.loadTestDurationMs = juce::Time::getMillisecondCounterHiRes() - loadTestStarted;
    result.loadTested = !loadError.has_value();
    result.loadTestMessage =
        loadError.value_or("VST3 instance created and initialized successfully.");
    result.scanDurationMs = juce::Time::getMillisecondCounterHiRes() - started;
    writeJson(riffra::encodePluginScanResult(result));
    return 0;
}

int validateLoad(const juce::String& path) {
    const auto started = juce::Time::getMillisecondCounterHiRes();
    if (!juce::File(path).exists()) {
        writeJson(makeLoadTestResult(path, false, "VST3 bundle or file does not exist.", 0.0));
        return 2;
    }

    const auto loadError = validateInstanceCreation(path);
    const auto durationMs = juce::Time::getMillisecondCounterHiRes() - started;
    if (loadError.has_value()) {
        writeJson(makeLoadTestResult(path, false, *loadError, durationMs));
        return 4;
    }

    writeJson(makeLoadTestResult(path, true, "VST3 instance created and initialized successfully.",
                                 durationMs));
    return 0;
}

}  // namespace

int main(int argc, char* argv[]) {
    if (!riffra::isolateProtocolChannel()) {
        writeJson(makeError({}, "Could not isolate the protocol channel on standard output."));
        return 1;
    }
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (argc < 2) {
        writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
        return 1;
    }
    const auto mode = juce::String(argv[1]);
    if (mode == "--help" || mode == "-h") {
        writeJson(makeError({},
                            "Usage: riffra-plugin-scan --scan <vst3-path>\n"
                            "       riffra-plugin-scan --validate-load <vst3-path>"));
        return 0;
    }
    if (argc != 3) {
        writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
        return 1;
    }
    const auto path = juce::String::fromUTF8(argv[2]);
    if (mode == "--scan") return scan(path);
    if (mode == "--validate-load") return validateLoad(path);
    writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
    return 1;
}
