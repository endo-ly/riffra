#pragma once

#include <JuceHeader.h>

namespace riffra::test {

inline juce::File builtInPresetRoot() { return juce::File(RIFFRA_SONALLOY_TEST_PRESET_ROOT); }

inline juce::File builtInPresetDirectory(const juce::String& presetId) {
    const auto manifest =
        juce::JSON::parse(builtInPresetRoot().getChildFile("manifest.json").loadFileAsString());
    const auto presets = manifest.getProperty("presets", {});
    if (!presets.isArray()) return {};

    for (const auto& preset : *presets.getArray()) {
        if (!preset.isObject() || preset.getProperty("id", {}).toString() != presetId) continue;

        const auto resourceBasePath = preset.getProperty("resourceBasePath", {}).toString();
        if (resourceBasePath.isEmpty()) return {};
        return builtInPresetRoot().getChildFile(resourceBasePath);
    }

    return {};
}

}  // namespace riffra::test
