#include <JuceHeader.h>
#include "PluginRack.h"

#include <iostream>
#include <optional>

namespace {

void writeJson(const juce::var& value) {
    std::cout << juce::JSON::toString(value, true) << std::endl;
}

juce::var makeError(const juce::String& path, const juce::String& message) {
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginScanError");
    result->setProperty("path", path);
    result->setProperty("message", message);
    result->setProperty("dataSafe", true);
    return juce::var(result);
}

juce::var describePlugin(const juce::PluginDescription& description) {
    auto* plugin = new juce::DynamicObject();
    plugin->setProperty("name", description.name);
    plugin->setProperty("descriptiveName", description.descriptiveName);
    plugin->setProperty("vendor", description.manufacturerName);
    plugin->setProperty("version", description.version);
    plugin->setProperty("category", description.category);
    plugin->setProperty("format", description.pluginFormatName);
    plugin->setProperty("path", description.fileOrIdentifier);
    plugin->setProperty("identifier", description.createIdentifierString());
    plugin->setProperty("uniqueId", static_cast<juce::int64>(description.uniqueId));
    plugin->setProperty("deprecatedUid", static_cast<juce::int64>(description.deprecatedUid));
    plugin->setProperty("numInputs", description.numInputChannels);
    plugin->setProperty("numOutputs", description.numOutputChannels);
    plugin->setProperty("isInstrument", description.isInstrument);
    plugin->setProperty("hasSharedContainer", description.hasSharedContainer);
    plugin->setProperty(
        "lastFileModifiedMs",
        static_cast<juce::int64>(description.lastFileModTime.toMilliseconds()));
    plugin->setProperty(
        "lastInfoUpdatedMs",
        static_cast<juce::int64>(description.lastInfoUpdateTime.toMilliseconds()));
    return juce::var(plugin);
}

juce::var makeLoadTestResult(
    const juce::String& path,
    bool success,
    const juce::String& message,
    double durationMs) {
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginLoadTestResult");
    result->setProperty("path", path);
    result->setProperty("success", success);
    result->setProperty("message", message);
    result->setProperty("durationMs", durationMs);
    result->setProperty("dataSafe", true);
    return juce::var(result);
}

std::optional<juce::String> validateInstanceCreation(const juce::String& path) {
    riffra::PluginRack rack;
    if (const auto loadError = rack.load(path, 44100.0, 512))
        return loadError->message;
    rack.clear();
    return std::nullopt;
}

int scan(const juce::String& path, bool includeLoadTest) {
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

    juce::Array<juce::var> plugins;
    for (const auto* description : descriptions)
        if (description != nullptr)
            plugins.add(describePlugin(*description));

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginScanResult");
    result->setProperty("path", path);
    result->setProperty("plugins", plugins);

    if (includeLoadTest) {
        const auto loadTestStarted = juce::Time::getMillisecondCounterHiRes();
        const auto loadError = validateInstanceCreation(path);
        const auto loadDurationMs =
            juce::Time::getMillisecondCounterHiRes() - loadTestStarted;
        result->setProperty("loadTested", loadError == std::nullopt);
        result->setProperty(
            "loadTestMessage",
            loadError.value_or("VST3 instance created and initialized successfully."));
        result->setProperty("loadTestDurationMs", loadDurationMs);
    }

    result->setProperty(
        "scanDurationMs",
        juce::Time::getMillisecondCounterHiRes() - started);
    writeJson(juce::var(result));
    return 0;
}

int validateLoad(const juce::String& path) {
    const auto started = juce::Time::getMillisecondCounterHiRes();
    if (!juce::File(path).exists()) {
        writeJson(makeLoadTestResult(
            path, false, "VST3 bundle or file does not exist.", 0.0));
        return 2;
    }

    const auto loadError = validateInstanceCreation(path);
    const auto durationMs = juce::Time::getMillisecondCounterHiRes() - started;
    if (loadError.has_value()) {
        writeJson(makeLoadTestResult(path, false, *loadError, durationMs));
        return 4;
    }

    writeJson(makeLoadTestResult(
        path, true, "VST3 instance created and initialized successfully.", durationMs));
    return 0;
}

} // namespace

int main(int argc, char* argv[]) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (argc < 2) {
        writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
        return 1;
    }
    const auto mode = juce::String(argv[1]);
    if (mode == "--help" || mode == "-h") {
        writeJson(makeError(
            {},
            "Usage: riffra-plugin-scan --scan <vst3-path>\n"
            "       riffra-plugin-scan --validate-load <vst3-path>"));
        return 0;
    }
    if (argc != 3) {
        writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
        return 1;
    }
    const auto path = juce::String::fromUTF8(argv[2]);
    if (mode == "--scan")
        return scan(path, true);
    if (mode == "--validate-load")
        return validateLoad(path);
    writeJson(makeError({}, "Usage: riffra-plugin-scan --scan|--validate-load <vst3-path>"));
    return 1;
}
