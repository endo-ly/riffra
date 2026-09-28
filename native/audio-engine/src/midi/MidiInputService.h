#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <mutex>
#include <set>
#include <vector>

namespace riffra {

class PreviewEngine;
class TimelineEngine;

class MidiMonitor final {
public:
    // Control thread only. The callback reads the installed targets without
    // taking a lock.
    void setPreviewEngine(PreviewEngine* preview) noexcept;
    void setTimelineEngine(TimelineEngine* engine) noexcept;

    // JUCE MIDI callback threads. This path must remain non-blocking.
    /// Routes one message from the device with the given MidiSourceRegistry index.
    void receive(std::uint16_t sourceIndex, const juce::MidiMessage& message);

    void setActive(bool value) noexcept;
    [[nodiscard]] bool isActive() const noexcept;
    [[nodiscard]] std::uint64_t getMessageCount() const noexcept;
    [[nodiscard]] int getLastNote() const noexcept;

private:
    std::atomic<bool> active{false};
    std::atomic<std::uint64_t> messageCount{0};
    std::atomic<int> lastNote{-1};
    PreviewEngine* previewEngine = nullptr;
    TimelineEngine* timelineEngine = nullptr;
};

class MidiInputService final {
public:
    // Control thread only. MIDI device open/close is never performed from an
    // audio callback.
    MidiInputService(PreviewEngine& previewEngine, TimelineEngine& timelineEngine);
    ~MidiInputService();

    MidiInputService(const MidiInputService&) = delete;
    MidiInputService& operator=(const MidiInputService&) = delete;

    [[nodiscard]] MidiMonitor& monitor() noexcept;
    void setListening(bool value) noexcept;
    [[nodiscard]] bool isListening() const noexcept;
    void reopenAll();
    [[nodiscard]] bool deviceSetChanged() const;

private:
    /// Forwards one opened device to the monitor together with its source index.
    class SourceCallback final : public juce::MidiInputCallback {
    public:
        SourceCallback(MidiMonitor& monitorIn, std::uint16_t sourceIndexIn) noexcept;
        void handleIncomingMidiMessage(juce::MidiInput*, const juce::MidiMessage& message) override;

    private:
        MidiMonitor& monitor;
        std::uint16_t sourceIndex;
    };

    struct OpenInput final {
        std::unique_ptr<SourceCallback> callback;
        // Declared after the callback so the device stops before it is destroyed.
        std::unique_ptr<juce::MidiInput> input;
    };

    TimelineEngine& timeline;
    MidiMonitor midiMonitor;
    mutable std::mutex inputsLock;
    std::vector<OpenInput> inputs;
    std::atomic<bool> listening{false};
    std::set<juce::String> activeDeviceIds;
};

}  // namespace riffra
