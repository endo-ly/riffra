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

    void audioProcessorParameterChanged(juce::AudioProcessor*, const int parameterIndex,
                                        const float newValue) override {
        if (!host.rack.hostEditInProgress.load(std::memory_order_acquire) &&
            !host.rack.isAutomationActive())
            host.queueParameterChange(parameterIndex, newValue);
    }

    void audioProcessorChanged(juce::AudioProcessor*, const ChangeDetails& details) override {
        if (host.rack.isAutomationActive() ||
            host.rack.hostEditInProgress.load(std::memory_order_acquire) ||
            (!details.nonParameterStateChanged && !details.programChanged))
            return;
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
            if (self->closeOnMessageThread() && self->onClosed) self->onClosed();
        });
    }

private:
    PluginEditorHost& host;
};

PluginEditorHost::PluginEditorHost(PluginRack& pluginRack, StateCallback stateCallback,
                                   ClosingStateCallback closingStateCallback,
                                   ParameterCallback parameterCallback,
                                   ClosedCallback closedCallback,
                                   std::shared_ptr<PluginRack> ownedRack)
    : rack(pluginRack),
      rackOwner(std::move(ownedRack)),
      onStateChanged(std::move(stateCallback)),
      onClosingState(std::move(closingStateCallback)),
      onParameterChanged(std::move(parameterCallback)),
      onClosed(std::move(closedCallback)),
      listener(std::make_unique<ProcessorListener>(*this)) {
    closingCompleted.signal();
    resizeParameterQueue();
}

PluginEditorHost::~PluginEditorHost() {
    auto* messageManager = juce::MessageManager::getInstanceWithoutCreating();
    if (messageManager == nullptr) {
        if (window != nullptr) std::_Exit(125);
        return;
    }
    if (messageManager->isThisTheMessageThread()) {
        (void)closeOnMessageThread();
        return;
    }
    // The owner must close the editor before releasing the last shared_ptr.
    // Deferring a raw `this` capture from a destructor would allow a late
    // Message Thread callback to access freed state.
    if (window != nullptr) std::_Exit(125);
}

bool PluginEditorHost::open(juce::String& error) {
    const auto result = std::make_shared<juce::String>();
    const auto self = shared_from_this();
    if (!runOnMessageThread([self, result] { self->openOnMessageThread(*result); }, error))
        return false;
    error = *result;
    return error.isEmpty();
}

bool PluginEditorHost::close() {
    juce::String ignored;
    const auto self = shared_from_this();
    const auto closed = std::make_shared<std::atomic<bool>>(false);
    if (!runOnMessageThread(
            [self, closed] {
                closed->store(self->closeOnMessageThread(), std::memory_order_release);
            },
            ignored) ||
        !closed->load(std::memory_order_acquire)) {
        // A plugin editor is third-party code. Once its Message Thread
        // boundary stops responding, destroying it from another thread is
        // unsafe; the sidecar is the recovery boundary.
        std::_Exit(125);
    }
    return true;
}

bool PluginEditorHost::waitForClosingState(const int timeoutMs) const {
    return closingCompleted.wait(timeoutMs) && closingSucceeded.load(std::memory_order_acquire);
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
    if (window == nullptr) return true;
    stateTimer.stop();
    if (listener != nullptr) rack.removeProcessorListener(*listener);
    drainParameterChanges();
    try {
        FaultInjection::before(FaultStage::destroy);
    } catch (...) {
        return false;
    }
    window.reset();
    if (onClosingState && rack.isAutomationActive()) {
        closingSucceeded.store(false, std::memory_order_release);
        closingCompleted.reset();
        pendingClosingState = true;
        deferredOwner = shared_from_this();
        stateTimer.start();
    } else {
        publishClosingState();
    }
    return true;
}

void PluginEditorHost::publishClosingState() {
    opaqueStateDirty.store(false, std::memory_order_release);
    if (!onClosingState || rack.isAutomationActive()) return;
    // Plugins may commit edits while their editor is destroyed and may change their state without
    // notifying the host, so the closing state is captured from the plugin itself.
    rack.staticStateDirty.store(true, std::memory_order_release);
    juce::String error;
    const auto state = rack.persistedState(error);
    onClosingState(state, error);
    closingSucceeded.store(state.has_value(), std::memory_order_release);
    closingCompleted.signal();
}

void PluginEditorHost::queueParameterChange(const int index, const float value) noexcept {
    if (index < 0 || static_cast<std::size_t>(index) >= parameterCapacity ||
        parameterValues == nullptr || parameterDirty == nullptr)
        return;
    if (rack.isAutomationActive()) return;
    rack.staticStateDirty.store(true, std::memory_order_release);
    const auto offset = static_cast<std::size_t>(index);
    parameterValues[offset].store(juce::jlimit(0.0f, 1.0f, value), std::memory_order_release);
    parameterDirty[offset].store(true, std::memory_order_release);
}

void PluginEditorHost::markOpaqueStateDirty() noexcept {
    if (rack.isAutomationActive()) return;
    rack.staticStateDirty.store(true, std::memory_order_release);
    lastOpaqueStateChangeMs.store(juce::Time::getMillisecondCounter(), std::memory_order_release);
    opaqueStateDirty.store(true, std::memory_order_release);
}

void PluginEditorHost::drainParameterChanges() {
    if (window != nullptr && window->getContentComponent() != nullptr)
        window->getContentComponent()->setEnabled(!rack.isAutomationActive());
    if (rack.isAutomationActive()) return;
    if (!onParameterChanged) return;
    for (std::size_t index = 0; index < parameterCapacity; ++index) {
        if (!parameterDirty[index].exchange(false, std::memory_order_acq_rel)) continue;
        onParameterChanged(static_cast<int>(index),
                           parameterValues[index].load(std::memory_order_acquire));
    }
}

void PluginEditorHost::resizeParameterQueue() noexcept {
    const auto count = rack.parameterCount();
    if (count == parameterCapacity) return;
    auto values =
        std::unique_ptr<std::atomic<float>[]>(new (std::nothrow) std::atomic<float>[count]);
    auto dirty = std::unique_ptr<std::atomic<bool>[]>(new (std::nothrow) std::atomic<bool>[count]);
    if (count > 0 && (values == nullptr || dirty == nullptr)) return;
    for (std::size_t index = 0; index < count; ++index) {
        values[index].store(0.0f, std::memory_order_relaxed);
        dirty[index].store(false, std::memory_order_relaxed);
    }
    parameterValues = std::move(values);
    parameterDirty = std::move(dirty);
    parameterCapacity = count;
}

void PluginEditorHost::publishStateIfDirty() {
    if (rack.isAutomationActive()) return;
    if (!onStateChanged || !opaqueStateDirty.load(std::memory_order_acquire)) return;
    const auto now = juce::Time::getMillisecondCounter();
    const auto changedAt = lastOpaqueStateChangeMs.load(std::memory_order_acquire);
    if (static_cast<std::uint32_t>(now - changedAt) < 400) return;
    opaqueStateDirty.store(false, std::memory_order_release);
    onStateChanged();
}

}  // namespace riffra
