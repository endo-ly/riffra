#include "MidiInputService.h"

#include "SafetyAudioCallback.h"
#include "TimelineEngine.h"

namespace riffra {

void MidiMonitor::setAudioCallback(SafetyAudioCallback* const callback) noexcept {
    audioCallback = callback;
}

void MidiMonitor::setTimelineEngine(TimelineEngine* const engine) noexcept {
    timelineEngine = engine;
}

void MidiMonitor::handleIncomingMidiMessage(juce::MidiInput* source,
                                            const juce::MidiMessage& message) {
    messageCount.fetch_add(1, std::memory_order_relaxed);
    const auto routedToTimeline =
        timelineEngine != nullptr &&
        timelineEngine->enqueueLiveMidi(
            message, source != nullptr ? source->getIdentifier() : juce::String{});
    if (routedToTimeline) {
        if (message.isNoteOn() || message.isNoteOff())
            lastNote.store(message.getNoteNumber(), std::memory_order_release);
        return;
    }
    if (!message.isNoteOn() && !message.isNoteOff()) return;

    lastNote.store(message.getNoteNumber(), std::memory_order_release);

    if (message.isNoteOff()) {
        if (audioCallback != nullptr) audioCallback->stopSynthNote(message.getNoteNumber());
        return;
    }

    if (audioCallback != nullptr)
        audioCallback->startSynthNote(message.getNoteNumber(), message.getFloatVelocity());
}

void MidiMonitor::setActive(const bool value) noexcept {
    active.store(value, std::memory_order_release);
}

bool MidiMonitor::isActive() const noexcept { return active.load(std::memory_order_acquire); }

std::uint64_t MidiMonitor::getMessageCount() const noexcept {
    return messageCount.load(std::memory_order_acquire);
}

int MidiMonitor::getLastNote() const noexcept { return lastNote.load(std::memory_order_acquire); }

MidiInputService::MidiInputService(SafetyAudioCallback& audioCallback,
                                   TimelineEngine& timelineEngine) {
    midiMonitor.setAudioCallback(&audioCallback);
    midiMonitor.setTimelineEngine(&timelineEngine);
}

MidiInputService::~MidiInputService() {
    setListening(false);
    reopenAll();
}

MidiMonitor& MidiInputService::monitor() noexcept { return midiMonitor; }

const MidiMonitor& MidiInputService::monitor() const noexcept { return midiMonitor; }

void MidiInputService::setListening(const bool value) noexcept {
    listening.store(value, std::memory_order_release);
}

bool MidiInputService::isListening() const noexcept {
    return listening.load(std::memory_order_acquire);
}

void MidiInputService::reopenAll() {
    const std::lock_guard lock(inputsLock);
    for (auto& input : inputs) {
        if (input != nullptr) input->stop();
    }
    inputs.clear();
    activeDeviceIds.clear();
    if (!isListening()) return;
    for (const auto& device : juce::MidiInput::getAvailableDevices()) {
        try {
            auto input = juce::MidiInput::openDevice(device.identifier, &midiMonitor);
            if (input == nullptr) continue;
            input->start();
            activeDeviceIds.insert(device.identifier);
            inputs.push_back(std::move(input));
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
