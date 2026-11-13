#include <JuceHeader.h>

#include <cmath>
#include <optional>

#include "contract/SidecarMessages.h"
#include "plugins/PluginRack.h"
#include "protocol/ProtocolChannel.h"

namespace {

constexpr auto kUsage =
    "Usage: riffra-plugin-scan --scan <vst3-path>\n"
    "       riffra-plugin-scan --validate-load <vst3-path>\n"
    "       riffra-plugin-scan --probe-sidechain <vst3-path>\n"
    "       riffra-plugin-scan --describe-parameters <vst3-path> <request.json>";

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
            rack.load(path, 44100.0, 512, riffra::PluginProcessingMode::realtime, false))
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

int probeSidechainCapability(const juce::String& path) {
    if (!juce::File(path).exists()) {
        writeJson(makeError(path, "VST3 bundle or file does not exist."));
        return 2;
    }

    riffra::PluginRack rack;
    if (const auto loadError =
            rack.load(path, 44100.0, 512, riffra::PluginProcessingMode::realtime, false)) {
        writeJson(makeError(path, loadError->message));
        return 4;
    }

    const auto capability = rack.status().sidechain;
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginSidechainCapability");
    result->setProperty("path", path);
    auto* sidechain = new juce::DynamicObject();
    sidechain->setProperty("available", capability.available);
    sidechain->setProperty("mono", capability.mono);
    sidechain->setProperty("stereo", capability.stereo);
    sidechain->setProperty("channels", capability.channels);
    sidechain->setProperty(
        "reason", capability.reason.has_value() ? juce::var(*capability.reason) : juce::var());
    result->setProperty("sidechain", juce::var(sidechain));
    writeJson(juce::var(result));
    return 0;
}

int describeParameters(const juce::String& path, const juce::String& requestPath) {
    juce::var request;
    const auto parsed = juce::JSON::parse(juce::File(requestPath).loadFileAsString(), request);
    if (parsed.failed() || !request.isObject()) {
        writeJson(makeError(path, "Parameter request is not a JSON object."));
        return 2;
    }
    auto* object = request.getDynamicObject();
    const auto values = object->getProperty("parameterValues");
    const auto data = object->getProperty("stateData");
    const auto displays = object->getProperty("displayRequests");
    if (!values.isArray() || (!data.isVoid() && !data.isString()) || !displays.isArray()) {
        writeJson(makeError(path, "Parameter request has invalid state or display requests."));
        return 2;
    }
    riffra::PluginStateSpec state;
    if (data.isString()) state.stateData = data.toString();
    for (const auto& item : *values.getArray()) {
        if ((!item.isDouble() && !item.isInt() && !item.isInt64()) ||
            !std::isfinite(static_cast<double>(item)) || static_cast<double>(item) < 0.0 ||
            static_cast<double>(item) > 1.0) {
            writeJson(makeError(path, "Parameter state contains an invalid normalized value."));
            return 2;
        }
        state.parameterValues.push_back(static_cast<float>(static_cast<double>(item)));
    }
    riffra::PluginRack rack;
    if (const auto error =
            rack.load(path, 44100.0, 512, riffra::PluginProcessingMode::realtime, false)) {
        writeJson(makeError(path, error->message));
        return 4;
    }
    juce::String error;
    if ((state.stateData.has_value() || !state.parameterValues.empty()) &&
        !rack.applyPersistedState(state, error)) {
        writeJson(makeError(path, error));
        return 4;
    }
    // Persisted parameter writes are queued for the audio owner. Flush that queue
    // before reading the plugin's parameter values and display metadata.
    rack.setBypassed(true);
    juce::AudioBuffer<float> scratch(1, 512);
    rack.process(nullptr, 0, nullptr, 0, scratch.getArrayOfWritePointers(), 1, 512);
    const auto parameters = rack.parameters();
    const auto captured = rack.persistedState(error);
    if (!captured.has_value()) {
        writeJson(makeError(path, error));
        return 4;
    }
    juce::Array<juce::var> currentValues;
    for (const auto value : captured->parameterValues) currentValues.add(juce::var(value));
    juce::Array<juce::var> encoded;
    for (const auto& parameter : parameters) {
        auto* item = new juce::DynamicObject();
        item->setProperty("index", parameter.index);
        item->setProperty("name", parameter.name);
        juce::Array<juce::var> choices;
        for (const auto& choice : parameter.choices) {
            auto* entry = new juce::DynamicObject();
            entry->setProperty("value", choice.value);
            entry->setProperty("displayValue", choice.displayValue);
            choices.add(juce::var(entry));
        }
        item->setProperty("choices", juce::var(choices));
        encoded.add(juce::var(item));
    }
    juce::Array<juce::var> conversions;
    for (const auto& display : *displays.getArray()) {
        auto* query = display.getDynamicObject();
        if (query == nullptr || !query->getProperty("name").isString() ||
            !query->getProperty("display").isString()) {
            writeJson(makeError(path, "Invalid parameter display request."));
            return 2;
        }
        const auto name = query->getProperty("name").toString();
        const auto requestedIndex = query->getProperty("index");
        if (!requestedIndex.isVoid() && !requestedIndex.isInt()) {
            writeJson(makeError(path, "Invalid parameter display index."));
            return 2;
        }
        juce::Array<juce::var> candidates;
        for (const auto& parameter : parameters) {
            const auto matchesIndex =
                requestedIndex.isVoid() || parameter.index == static_cast<int>(requestedIndex);
            if (parameter.name != name || !matchesIndex) continue;
            juce::String conversionError;
            const auto text = query->getProperty("display").toString();
            const auto value = rack.resolveParameterDisplay(parameter.index, text, conversionError);
            auto* candidate = new juce::DynamicObject();
            candidate->setProperty("index", parameter.index);
            candidate->setProperty("value", value ? juce::var(*value) : juce::var());
            candidate->setProperty("error", value ? juce::var() : juce::var(conversionError));
            candidates.add(juce::var(candidate));
        }
        conversions.add(juce::var(candidates));
    }
    auto* result = new juce::DynamicObject();
    result->setProperty("type", "pluginParameters");
    result->setProperty("path", path);
    result->setProperty("parameters", juce::var(encoded));
    result->setProperty("parameterValues", juce::var(currentValues));
    result->setProperty("conversions", juce::var(conversions));
    writeJson(juce::var(result));
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
        writeJson(makeError({}, kUsage));
        return 1;
    }
    const auto mode = juce::String(argv[1]);
    if (mode == "--help" || mode == "-h") {
        writeJson(makeError({}, kUsage));
        return 0;
    }
    if (mode == "--describe-parameters" && argc == 4)
        return describeParameters(juce::String::fromUTF8(argv[2]), juce::String::fromUTF8(argv[3]));
    if (argc != 3) {
        writeJson(makeError({}, kUsage));
        return 1;
    }
    const auto path = juce::String::fromUTF8(argv[2]);
    if (mode == "--scan") return scan(path);
    if (mode == "--validate-load") return validateLoad(path);
    if (mode == "--probe-sidechain") return probeSidechainCapability(path);
    writeJson(makeError({}, kUsage));
    return 1;
}
