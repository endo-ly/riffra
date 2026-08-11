#include "PluginEditorHost.h"

#include <cstdlib>
#include <exception>
#include <mutex>
#include <new>

#include "FaultInjection.h"
#include "PluginRack.h"

namespace riffra {

class PluginEditorHost::ProcessorListener final : public juce::AudioProcessorListener {
public:
    explicit ProcessorListener(PluginEditorHost& target) : host(target) {}

    void audioProcessorParameterChanged(
        juce::AudioProcessor*, const int parameterIndex, const float newValue) override {
        host.queueParameterChange(parameterIndex, newValue);
    }

    void audioProcessorChanged(
        juce::AudioProcessor*, const ChangeDetails&) override {
        host.markOpaqueStateDirty();
    }

private:
    PluginEditorHost& host;
};

class PluginEditorHost::EditorWindow final : public juce::DocumentWindow {
public:
    EditorWindow(const juce::String& title, std::unique_ptr<juce::AudioProcessorEditor> editor,
                 PluginEditorHost& owner)
        : DocumentWindow(title, juce::Colours::black, juce::DocumentWindow::closeButton),
          host(owner) {
        auto* editorView = editor.get();
        setUsingNativeTitleBar(true);
        setContentOwned(editor.release(), true);
        setResizable(editorView->isResizable(), false);
        centreWithSize(getWidth(), getHeight());
        setVisible(true);
    }

    void closeButtonPressed() override {
        auto self = host.shared_from_this();
        juce::MessageManager::callAsync([self = std::move(self)] {
            (void) self->closeOnMessageThread();
        });
    }

private:
    PluginEditorHost& host;
};

PluginEditorHost::PluginEditorHost(
    PluginRack& pluginRack,
    StateCallback stateCallback,
    ParameterCallback parameterCallback)
    : rack(pluginRack),
      onStateChanged(std::move(stateCallback)),
      onParameterChanged(std::move(parameterCallback)),
      listener(std::make_unique<ProcessorListener>(*this)) {
    resizeParameterQueue();
}

PluginEditorHost::~PluginEditorHost() {
    auto* messageManager = juce::MessageManager::getInstanceWithoutCreating();
    if (messageManager == nullptr) {
        if (window != nullptr)
            std::_Exit(125);
        return;
    }
    if (messageManager->isThisTheMessageThread()) {
        (void) closeOnMessageThread();
        return;
    }
    // The owner must close the editor before releasing the last shared_ptr.
    // Deferring a raw `this` capture from a destructor would allow a late
    // Message Thread callback to access freed state.
    if (window != nullptr)
        std::_Exit(125);
}

bool PluginEditorHost::open(juce::String& error) {
    const auto result = std::make_shared<juce::String>();
    const auto self = shared_from_this();
    if (!runOnMessageThread(
            [self, result] { self->openOnMessageThread(*result); },
            error))
        return false;
    error = *result;
    return error.isEmpty();
}

bool PluginEditorHost::close() {
    juce::String ignored;
    const auto self = shared_from_this();
    const auto closed = std::make_shared<std::atomic<bool>>(false);
    if (!runOnMessageThread(
            [self, closed] { closed->store(self->closeOnMessageThread(), std::memory_order_release); },
            ignored)
        || !closed->load(std::memory_order_acquire)) {
        // A plugin editor is third-party code. Once its Message Thread
        // boundary stops responding, destroying it from another thread is
        // unsafe; the sidecar is the recovery boundary.
        std::_Exit(125);
    }
    return true;
}

std::optional<PluginLoadError> PluginEditorHost::load(const juce::String& path,
                                                       const double sampleRate,
                                                       const int blockSize,
                                                       const juce::var& persistedState) {
    struct LoadResult final {
        std::optional<PluginLoadError> value;
    };
    const auto result = std::make_shared<LoadResult>();
    const auto self = shared_from_this();
    juce::String dispatchError;
    if (!runOnMessageThread(
        [self, path, sampleRate, blockSize, persistedState, result] {
                if (!self->closeOnMessageThread()) {
                    result->value = PluginLoadError{
                        "pluginEditor",
                        "The previous VST3 editor could not be closed safely.",
                    };
                    return;
                }
                result->value = self->rack.load(path, sampleRate, blockSize);
                if (!result->value.has_value() && persistedState.isObject()) {
                    juce::String stateError;
                    if (!self->rack.applyPersistedState(persistedState, stateError)) {
                        self->rack.clear();
                        result->value = PluginLoadError{
                            "pluginState",
                            stateError.isNotEmpty()
                                ? stateError
                                : "The VST3 persisted state could not be applied.",
                        };
                    }
                }
                if (!result->value.has_value())
                    self->resizeParameterQueue();
            },
            dispatchError)) {
        return PluginLoadError{"pluginLifecycle", dispatchError};
    }
    return result->value;
}

bool PluginEditorHost::clear(juce::String& error) {
    const auto self = shared_from_this();
    const auto closed = std::make_shared<std::atomic<bool>>(false);
    if (!runOnMessageThread(
        [self, closed] {
            if (!self->closeOnMessageThread())
                return;
            closed->store(true, std::memory_order_release);
            self->rack.clear();
        },
        error))
        return false;
    if (!closed->load(std::memory_order_acquire)) {
        error = "The VST3 editor could not be closed safely.";
        std::_Exit(125);
    }
    return true;
}

bool PluginEditorHost::runOnMessageThread(std::function<void()> operation, juce::String& error) {
    auto* messageManager = juce::MessageManager::getInstanceWithoutCreating();
    if (messageManager == nullptr) {
        error = "The plugin editor message loop is unavailable.";
        return false;
    }
    if (messageManager->isThisTheMessageThread()) {
        operation();
        return true;
    }

    struct Dispatch final {
        std::function<void()> operation;
        juce::WaitableEvent completed;
        std::exception_ptr exception;
        std::mutex exceptionLock;
    };
    const auto dispatch = std::make_shared<Dispatch>();
    dispatch->operation = std::move(operation);
    if (!juce::MessageManager::callAsync([dispatch] {
            try {
                dispatch->operation();
            } catch (...) {
                const std::lock_guard lock(dispatch->exceptionLock);
                dispatch->exception = std::current_exception();
            }
            dispatch->completed.signal();
        })) {
        error = "The plugin editor command could not reach the message thread.";
        return false;
    }
    constexpr int kMessageThreadTimeoutMs = 15'000;
    if (!dispatch->completed.wait(kMessageThreadTimeoutMs)) {
        error = "The plugin editor message thread did not complete within 15 seconds.";
        return false;
    }
    {
        const std::lock_guard lock(dispatch->exceptionLock);
        if (dispatch->exception != nullptr) {
            error = "The plugin editor message-thread operation raised an exception.";
            return false;
        }
    }
    return true;
}

void PluginEditorHost::openOnMessageThread(juce::String& error) {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    if (window != nullptr) {
        window->toFront(true);
        return;
    }

    FaultInjection::before(FaultStage::editorOpen);
    std::unique_ptr<juce::AudioProcessorEditor> editor(rack.createEditor(error));
    if (editor == nullptr) {
        if (error.isEmpty()) error = "The loaded VST3 does not provide an editor.";
        return;
    }
    try {
        window = std::make_unique<EditorWindow>(rack.currentPluginName(), std::move(editor), *this);
        rack.addProcessorListener(*listener);
        stateTimer.start();
    } catch (const std::exception& exception) {
        error =
            "VST3 editor window creation raised an exception: " + juce::String(exception.what());
    } catch (...) {
        error = "VST3 editor window creation failed with an unknown exception.";
    }
}

bool PluginEditorHost::closeOnMessageThread() {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    if (window == nullptr)
        return true;
    stateTimer.stop();
    drainParameterChanges();
    publishStateIfDirty(true);
    if (listener != nullptr)
        rack.removeProcessorListener(*listener);
    try {
        FaultInjection::before(FaultStage::destroy);
    } catch (...) {
        return false;
    }
    window.reset();
    return true;
}

void PluginEditorHost::queueParameterChange(const int index, const float value) noexcept {
    if (index < 0 || static_cast<std::size_t>(index) >= parameterCapacity
        || parameterValues == nullptr || parameterDirty == nullptr)
        return;
    const auto offset = static_cast<std::size_t>(index);
    parameterValues[offset].store(juce::jlimit(0.0f, 1.0f, value), std::memory_order_release);
    parameterDirty[offset].store(true, std::memory_order_release);
    parameterStateDirty.store(true, std::memory_order_release);
}

void PluginEditorHost::markOpaqueStateDirty() noexcept {
    lastOpaqueStateChangeMs.store(juce::Time::getMillisecondCounter(), std::memory_order_release);
    opaqueStateDirty.store(true, std::memory_order_release);
}

void PluginEditorHost::drainParameterChanges() {
    if (!onParameterChanged)
        return;
    for (std::size_t index = 0; index < parameterCapacity; ++index) {
        if (!parameterDirty[index].exchange(false, std::memory_order_acq_rel))
            continue;
        onParameterChanged(
            static_cast<int>(index),
            parameterValues[index].load(std::memory_order_acquire));
    }
}

void PluginEditorHost::resizeParameterQueue() noexcept {
    const auto count = rack.parameterCount();
    if (count == parameterCapacity)
        return;
    auto values = std::unique_ptr<std::atomic<float>[]>(new (std::nothrow) std::atomic<float>[count]);
    auto dirty = std::unique_ptr<std::atomic<bool>[]>(new (std::nothrow) std::atomic<bool>[count]);
    if (count > 0 && (values == nullptr || dirty == nullptr))
        return;
    for (std::size_t index = 0; index < count; ++index) {
        values[index].store(0.0f, std::memory_order_relaxed);
        dirty[index].store(false, std::memory_order_relaxed);
    }
    parameterValues = std::move(values);
    parameterDirty = std::move(dirty);
    parameterCapacity = count;
}

void PluginEditorHost::publishStateIfDirty(const bool force) {
    const auto opaqueDirty = opaqueStateDirty.load(std::memory_order_acquire);
    const auto parameterStateChanged = parameterStateDirty.load(std::memory_order_acquire);
    if (!onStateChanged || (!opaqueDirty && !(force && parameterStateChanged)))
        return;
    const auto now = juce::Time::getMillisecondCounter();
    const auto changedAt = lastOpaqueStateChangeMs.load(std::memory_order_acquire);
    if (!force && static_cast<std::uint32_t>(now - changedAt) < 400)
        return;
    opaqueStateDirty.store(false, std::memory_order_release);
    parameterStateDirty.store(false, std::memory_order_release);
    juce::String error;
    const auto state = rack.persistedState(error);
    if (error.isNotEmpty() || !state.isObject())
        return;
    onStateChanged(state);
}

}  // namespace riffra
