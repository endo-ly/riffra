#include "PluginEditorHost.h"

#include <exception>

#include "PluginRack.h"

namespace riffra {

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

PluginEditorHost::PluginEditorHost(PluginRack& pluginRack) : rack(pluginRack) {}

PluginEditorHost::~PluginEditorHost() {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    window.reset();
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
    } catch (const std::exception& exception) {
        error =
            "VST3 editor window creation raised an exception: " + juce::String(exception.what());
    } catch (...) {
        error = "VST3 editor window creation failed with an unknown exception.";
    }
}

void PluginEditorHost::closeOnMessageThread() {
    jassert(juce::MessageManager::getInstance()->isThisTheMessageThread());
    window.reset();
}

}  // namespace riffra
