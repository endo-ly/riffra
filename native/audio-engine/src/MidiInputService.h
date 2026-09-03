#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <mutex>
#include <set>
#include <vector>

namespace riffra {

class SafetyAudioCallback;
class TimelineEngine;

class MidiMonitor final : public juce::MidiInputCallback {
public:
    void setAudioCallback(SafetyAudioCallback* callback) noexcept;
    void setTimelineEngine(TimelineEngine* engine) noexcept;

    void handleIncomingMidiMessage(juce::MidiInput* source,
                                   const juce::MidiMessage& message) override;

    void setActive(bool value) noexcept;
    [[nodiscard]] bool isActive() const noexcept;
    [[nodiscard]] std::uint64_t getMessageCount() const noexcept;
    [[nodiscard]] int getLastNote() const noexcept;

private:
    std::atomic<bool> active{false};
    std::atomic<std::uint64_t> messageCount{0};
    std::atomic<int> lastNote{-1};
    SafetyAudioCallback* audioCallback = nullptr;
    TimelineEngine* timelineEngine = nullptr;
};

class MidiInputService final {
public:
    MidiInputService(SafetyAudioCallback& audioCallback, TimelineEngine& timelineEngine);
    ~MidiInputService();

    MidiInputService(const MidiInputService&) = delete;
    MidiInputService& operator=(const MidiInputService&) = delete;

    [[nodiscard]] MidiMonitor& monitor() noexcept;
    [[nodiscard]] const MidiMonitor& monitor() const noexcept;
    void setListening(bool value) noexcept;
    [[nodiscard]] bool isListening() const noexcept;
    void reopenAll();
    [[nodiscard]] bool deviceSetChanged() const;

private:
    MidiMonitor midiMonitor;
    mutable std::mutex inputsLock;
    std::vector<std::unique_ptr<juce::MidiInput>> inputs;
    std::atomic<bool> listening{false};
    std::set<juce::String> activeDeviceIds;
};

}  // namespace riffra
