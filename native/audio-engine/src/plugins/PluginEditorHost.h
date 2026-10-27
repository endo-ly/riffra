#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <functional>
#include <memory>
#include <optional>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final : public std::enable_shared_from_this<PluginEditorHost> {
public:
    using StateCallback = std::function<void()>;
    /// Runs on the Message Thread with the state captured after the editor window was destroyed,
    /// or with the capture error.
    using ClosingStateCallback =
        std::function<void(const std::optional<PluginStateSpec>& state, const juce::String& error)>;
    using ParameterCallback = std::function<void(int, float)>;
    /// Runs on the Message Thread after the user closed the editor window.
    using ClosedCallback = std::function<void()>;

    explicit PluginEditorHost(PluginRack& rack, StateCallback stateCallback = {},
                              ClosingStateCallback closingStateCallback = {},
                              ParameterCallback parameterCallback = {},
                              ClosedCallback closedCallback = {},
                              std::shared_ptr<PluginRack> rackOwner = {});
    ~PluginEditorHost();

    bool open(juce::String& error);
    bool close();
    /// Waits for the closing snapshot after the audio owner restores automation.
    [[nodiscard]] bool waitForClosingState(int timeoutMs) const;
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
    void publishStateIfDirty();
    void publishClosingState();
    void resizeParameterQueue() noexcept;

    class StateTimer final : private juce::Timer {
    public:
        explicit StateTimer(PluginEditorHost& owner) : host(owner) {}
        void start() { startTimer(25); }
        void stop() { stopTimer(); }

    private:
        void timerCallback() override {
            auto keepAlive = host.shared_from_this();
            if (host.pendingClosingState && !host.rack.isAutomationActive()) {
                host.pendingClosingState = false;
                host.publishClosingState();
                if (host.window == nullptr) stop();
                host.deferredOwner.reset();
            }
            host.drainParameterChanges();
            host.publishStateIfDirty();
        }
        PluginEditorHost& host;
    };

    PluginRack& rack;
    std::shared_ptr<PluginRack> rackOwner;
    std::shared_ptr<PluginEditorHost> deferredOwner;
    bool pendingClosingState = false;
    mutable juce::WaitableEvent closingCompleted{true};
    std::atomic<bool> closingSucceeded{true};
    StateCallback onStateChanged;
    ClosingStateCallback onClosingState;
    ParameterCallback onParameterChanged;
    ClosedCallback onClosed;
    std::unique_ptr<std::atomic<float>[]> parameterValues;
    std::unique_ptr<std::atomic<bool>[]> parameterDirty;
    std::size_t parameterCapacity = 0;
    std::atomic<bool> opaqueStateDirty{false};
    std::atomic<std::uint32_t> lastOpaqueStateChangeMs{0};
    std::unique_ptr<ProcessorListener> listener;
    StateTimer stateTimer{*this};
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
