#include "MidiInputService.h"

#include "audio/PluginAudition.h"
#include "audio/PreviewEngine.h"
#include "timeline/TimelineEngine.h"

namespace riffra {

void MidiMonitor::setPreviewEngine(PreviewEngine* const preview) noexcept {
    previewEngine = preview;
}

void MidiMonitor::setTimelineEngine(TimelineEngine* const engine) noexcept {
    timelineEngine = engine;
}

void MidiMonitor::setAudition(PluginAudition* const audition) noexcept {
    pluginAudition = audition;
}

void MidiMonitor::receive(const std::uint16_t sourceIndex, const juce::MidiMessage& message) {
    messageCount.fetch_add(1, std::memory_order_relaxed);
    const auto routed =
        (pluginAudition != nullptr && pluginAudition->enqueueMidi(message)) ||
        (timelineEngine != nullptr && timelineEngine->enqueueLiveMidi(sourceIndex, message));
    if (routed) {
        if (message.isNoteOn() || message.isNoteOff())
            lastNote.store(message.getNoteNumber(), std::memory_order_release);
        return;
    }
    if (!message.isNoteOn() && !message.isNoteOff()) return;

    lastNote.store(message.getNoteNumber(), std::memory_order_release);

    if (message.isNoteOff()) {
        if (previewEngine != nullptr) previewEngine->stopSynthNote(message.getNoteNumber());
        return;
    }

    if (previewEngine != nullptr)
        previewEngine->startSynthNote(message.getNoteNumber(), message.getFloatVelocity());
}

void MidiMonitor::setActive(const bool value) noexcept {
    active.store(value, std::memory_order_release);
}

bool MidiMonitor::isActive() const noexcept { return active.load(std::memory_order_acquire); }

std::uint64_t MidiMonitor::getMessageCount() const noexcept {
    return messageCount.load(std::memory_order_acquire);
}

int MidiMonitor::getLastNote() const noexcept { return lastNote.load(std::memory_order_acquire); }

MidiInputService::SourceCallback::SourceCallback(MidiMonitor& monitorIn,
                                                 const std::uint16_t sourceIndexIn) noexcept
    : monitor(monitorIn), sourceIndex(sourceIndexIn) {}

void MidiInputService::SourceCallback::handleIncomingMidiMessage(juce::MidiInput*,
                                                                 const juce::MidiMessage& message) {
    monitor.receive(sourceIndex, message);
}

MidiInputService::MidiInputService(PreviewEngine& previewEngine, TimelineEngine& timelineEngine)
    : timeline(timelineEngine) {
    midiMonitor.setPreviewEngine(&previewEngine);
    midiMonitor.setTimelineEngine(&timelineEngine);
}

MidiInputService::~MidiInputService() {
    setListening(false);
    reopenAll();
}

MidiMonitor& MidiInputService::monitor() noexcept { return midiMonitor; }

void MidiInputService::setListening(const bool value) noexcept {
    listening.store(value, std::memory_order_release);
}

bool MidiInputService::isListening() const noexcept {
    return listening.load(std::memory_order_acquire);
}

void MidiInputService::reopenAll() {
    const std::lock_guard lock(inputsLock);
    for (auto& open : inputs) open.input->stop();
    inputs.clear();
    activeDeviceIds.clear();
    if (!isListening()) return;
    for (const auto& device : juce::MidiInput::getAvailableDevices()) {
        try {
            auto callback = std::make_unique<SourceCallback>(
                midiMonitor, timeline.midiSourceIndex(device.identifier));
            auto input = juce::MidiInput::openDevice(device.identifier, callback.get());
            if (input == nullptr) continue;
            input->start();
            activeDeviceIds.insert(device.identifier);
            inputs.push_back({std::move(callback), std::move(input)});
        } catch (...) {
            // A single MIDI device that fails to open must not block the others.
        }
    }
}

bool MidiInputService::deviceSetChanged() const {
    if (!isListening()) return false;
    std::set<juce::String> currentIds;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        currentIds.insert(device.identifier);
    const std::lock_guard lock(inputsLock);
    return currentIds != activeDeviceIds;
}

}  // namespace riffra
