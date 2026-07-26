#pragma once

#include <JuceHeader.h>

#include <functional>
#include <array>
#include <atomic>
#include <memory>
#include <optional>

#include "PluginRack.h"

namespace riffra {

class PluginEditorHost final {
public:
    using StateCallback = std::function<void(const juce::var&)>;
    using ParameterCallback = std::function<void(int, float)>;

    explicit PluginEditorHost(
        PluginRack& rack,
        StateCallback stateCallback = {},
        ParameterCallback parameterCallback = {});
    ~PluginEditorHost();

    bool open(juce::String& error);
    void close();
    [[nodiscard]] std::optional<PluginLoadError> load(const juce::String& path, double sampleRate,
                                                      int blockSize);
    bool clear(juce::String& error);

private:
    class EditorWindow;
    class ProcessorListener;

    bool runOnMessageThread(std::function<void()> operation, juce::String& error);
    void openOnMessageThread(juce::String& error);
    void closeOnMessageThread();
    void queueParameterChange(int index, float value) noexcept;
    void markOpaqueStateDirty() noexcept;
    void drainParameterChanges();
    void publishStateIfDirty(bool force);

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
    static constexpr std::size_t kParameterCapacity = 512;
    std::array<std::atomic<float>, kParameterCapacity> parameterValues {};
    std::array<std::atomic<bool>, kParameterCapacity> parameterDirty {};
    std::atomic<bool> opaqueStateDirty { false };
    std::atomic<bool> parameterStateDirty { false };
    std::atomic<std::uint32_t> lastOpaqueStateChangeMs { 0 };
    std::unique_ptr<ProcessorListener> listener;
    StateTimer stateTimer { *this };
    std::unique_ptr<EditorWindow> window;
};

}  // namespace riffra
