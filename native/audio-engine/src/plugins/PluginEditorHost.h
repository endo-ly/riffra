#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <functional>
#include <memory>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final : public std::enable_shared_from_this<PluginEditorHost> {
public:
    using StateCallback = std::function<void(const PluginStateSpec&)>;
    using ParameterCallback = std::function<void(int, float)>;
    /// Runs on the Message Thread after the user closed the editor window.
    using ClosedCallback = std::function<void()>;

    explicit PluginEditorHost(PluginRack& rack, StateCallback stateCallback = {},
                              ParameterCallback parameterCallback = {},
                              ClosedCallback closedCallback = {});
    ~PluginEditorHost();

    bool open(juce::String& error);
    bool close();
    /// Whether the editor window is showing. Message Thread only.
    [[nodiscard]] bool isOpen() const noexcept { return window != nullptr; }

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
    ClosedCallback onClosed;
    std::unique_ptr<std::atomic<float>[]> parameterValues;
    std::unique_ptr<std::atomic<bool>[]> parameterDirty;
    std::size_t parameterCapacity = 0;
    std::atomic<bool> opaqueStateDirty{false};
    std::atomic<bool> parameterStateDirty{false};
    std::atomic<std::uint32_t> lastOpaqueStateChangeMs{0};
    std::unique_ptr<ProcessorListener> listener;
    StateTimer stateTimer{*this};
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
