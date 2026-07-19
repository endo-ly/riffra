#pragma once

#include <JuceHeader.h>

#include <functional>
#include <memory>
#include <optional>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final {
public:
    explicit PluginEditorHost(PluginRack& rack);
    ~PluginEditorHost();

    bool open(juce::String& error);
    void close();
    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize);
    bool clear(juce::String& error);

private:
    class EditorWindow;

    bool runOnMessageThread(std::function<void()> operation, juce::String& error);
    void openOnMessageThread(juce::String& error);
    void closeOnMessageThread();

    PluginRack& rack;
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
