#include <JuceHeader.h>

#include <iostream>

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

    juce::Array<juce::var> plugins;
    for (const auto* description : descriptions)
        if (description != nullptr)
            plugins.add(describePlugin(*description));

    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginScanResult");
    result->setProperty("path", path);
    result->setProperty("plugins", plugins);
    result->setProperty(
        "scanDurationMs",
        juce::Time::getMillisecondCounterHiRes() - started);
    writeJson(juce::var(result));
    return 0;
}

} // namespace

int main(int argc, char* argv[]) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (argc != 3 || juce::String(argv[1]) != "--scan") {
        writeJson(makeError({}, "Usage: riffra-plugin-scan --scan <vst3-path>"));
        return 1;
    }
    return scan(juce::String::fromUTF8(argv[2]));
}
