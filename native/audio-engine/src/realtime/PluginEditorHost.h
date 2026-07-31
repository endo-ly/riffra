#pragma once

#include <JuceHeader.h>

#include <functional>
#include <atomic>
#include <memory>
#include <optional>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final : public std::enable_shared_from_this<PluginEditorHost> {
public:
    using StateCallback = std::function<void(const juce::var&)>;
    using ParameterCallback = std::function<void(int, float)>;

    explicit PluginEditorHost(
        PluginRack& rack,
        StateCallback stateCallback = {},
        ParameterCallback parameterCallback = {});
    ~PluginEditorHost();

    bool open(juce::String& error);
    bool close();
    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize,
                                                      const juce::var& persistedState = {});
    bool clear(juce::String& error);

private:
    class EditorWindow;
    class ProcessorListener;

    bool runOnMessageThread(std::function<void()> operation, juce::String& error);
    void openOnMessageThread(juce::String& error);
    bool closeOnMessageThread();
    void queueParameterChange(int index, float value) noexcept;
    void markOpaqueStateDirty() noexcept;
    void drainParameterChanges();
    void publishStateIfDirty(bool force);
    void resizeParameterQueue() noexcept;

    class StateTimer final : private juce::Timer {
    public:
        explicit StateTimer(PluginEditorHost& owner) : host(owner) {}
        void start() { startTimer(25); }
        void stop() { stopTimer(); }

    private:
        void timerCallback() override {
            host.drainParameterChanges();
            host.publishStateIfDirty(false);
        }
        PluginEditorHost& host;
    };

    PluginRack& rack;
    StateCallback onStateChanged;
    ParameterCallback onParameterChanged;
    std::unique_ptr<std::atomic<float>[]> parameterValues;
    std::unique_ptr<std::atomic<bool>[]> parameterDirty;
    std::size_t parameterCapacity = 0;
    std::atomic<bool> opaqueStateDirty { false };
    std::atomic<bool> parameterStateDirty { false };
    std::atomic<std::uint32_t> lastOpaqueStateChangeMs { 0 };
    std::unique_ptr<ProcessorListener> listener;
    StateTimer stateTimer { *this };
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
