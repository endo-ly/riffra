#pragma once

#include <JuceHeader.h>

#include <functional>
#include <memory>
#include <optional>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final {
public:
    using StateCallback = std::function<void(const juce::var&)>;

    explicit PluginEditorHost(PluginRack& rack, StateCallback stateCallback = {});
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
    void publishStateIfChanged(bool force);

    class StateTimer final : private juce::Timer {
    public:
        explicit StateTimer(PluginEditorHost& owner) : host(owner) {}
        void start() { startTimer(100); }
        void stop() { stopTimer(); }

    private:
        void timerCallback() override { host.publishStateIfChanged(false); }
        PluginEditorHost& host;
    };

    PluginRack& rack;
    StateCallback onStateChanged;
    juce::String lastPublishedState;
    StateTimer stateTimer { *this };
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
