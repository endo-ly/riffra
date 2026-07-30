#include "PluginEditorHost.h"

#include <exception>
#include <new>

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
        juce::MessageManager::callAsync([&owner = host] { owner.closeOnMessageThread(); });
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
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    closeOnMessageThread();
}

bool PluginEditorHost::open(juce::String& error) {
    return runOnMessageThread([this, &error] { openOnMessageThread(error); }, error) &&
           error.isEmpty();
}

void PluginEditorHost::close() {
    juce::String ignored;
    runOnMessageThread([this] { closeOnMessageThread(); }, ignored);
}

std::optional<PluginLoadError> PluginEditorHost::load(const juce::String& path,
                                                       const double sampleRate,
                                                       const int blockSize) {
    std::optional<PluginLoadError> result;
    juce::String dispatchError;
    if (!runOnMessageThread(
            [this, &path, sampleRate, blockSize, &result] {
                closeOnMessageThread();
                result = rack.load(path, sampleRate, blockSize);
                if (!result.has_value())
                    resizeParameterQueue();
            },
            dispatchError)) {
        return PluginLoadError{"pluginLifecycle", dispatchError};
    }
    return result;
}

bool PluginEditorHost::clear(juce::String& error) {
    return runOnMessageThread(
        [this] {
            closeOnMessageThread();
            rack.clear();
        },
        error);
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

    juce::WaitableEvent completed;
    if (!juce::MessageManager::callAsync([operation = std::move(operation), &completed] {
            operation();
            completed.signal();
        })) {
        error = "The plugin editor command could not reach the message thread.";
        return false;
    }
    completed.wait();
    return true;
}

void PluginEditorHost::openOnMessageThread(juce::String& error) {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    if (window != nullptr) {
        window->toFront(true);
        return;
    }

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

void PluginEditorHost::closeOnMessageThread() {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    stateTimer.stop();
    drainParameterChanges();
    publishStateIfDirty(true);
    if (listener != nullptr)
        rack.removeProcessorListener(*listener);
    window.reset();
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
