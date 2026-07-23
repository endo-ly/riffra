#pragma once

#include "ArrangementCaptureSink.h"
#include "RecordingSession.h"

#include <array>
#include <atomic>
#include <memory>
#include <limits>
#include <vector>

namespace riffra {

class ArrangeRecordingSession final : public ArrangementCaptureSink {
public:
    static std::unique_ptr<ArrangeRecordingSession> create(
        const juce::File& directory,
        const juce::var& configuration,
        juce::String& error);

    void writeAudioTrack(
        const juce::String& trackId,
        const float* raw,
        const float* const* processed,
        int sampleCount) noexcept override;
    void markLoopBoundary(std::uint64_t audioSample) noexcept override;
    void writeMidiTrack(
        const juce::String& trackId,
        const juce::String& sourceDeviceId,
        const juce::MidiMessage& message,
        std::uint64_t audioSample) noexcept override;
    void setCaptureRange(
        std::uint64_t startAudioSample,
        std::uint64_t endAudioSample) noexcept override;
    bool finish(juce::String& error);
    [[nodiscard]] juce::var status() const;

private:
    struct TrackWriter final {
        struct MidiEvent final {
            std::uint64_t audioSample = 0;
            juce::String sourceDeviceId;
            int status = 0;
            int channel = 0;
            int data1 = 0;
            int data2 = 0;
        };
        juce::String trackId;
        juce::String trackKey;
        juce::String kind;
        int audioInputChannel = -1;
        juce::String midiDeviceId;
        int midiChannel = 0;
        int pluginLatencySamples = 0;
        std::unique_ptr<RecordingSession> audio;
        std::vector<MidiEvent> midiEvents;
    };

    ArrangeRecordingSession(juce::File directory, double sampleRate);
    bool initialise(const juce::var& configuration, juce::String& error);
    bool writeManifest(const juce::String& state, juce::String& error) const;

    static constexpr std::size_t kMaximumLoopBoundaries = 4096;
    juce::File directory;
    juce::File manifest;
    double sampleRate = 0.0;
    std::uint64_t timelineStartTick = 0;
    bool loopEnabled = false;
    std::int64_t loopStartSample = 0;
    std::int64_t loopEndSample = 0;
    bool punchEnabled = false;
    std::int64_t punchStartSample = 0;
    std::int64_t punchEndSample = 0;
    std::vector<TrackWriter> tracks;
    mutable juce::CriticalSection midiLock;
    std::array<std::atomic<std::uint64_t>, kMaximumLoopBoundaries> loopBoundaries {};
    std::atomic<std::size_t> loopBoundaryCount { 0 };
    std::atomic<std::uint64_t> recordStartAudioSample {
        std::numeric_limits<std::uint64_t>::max()
    };
    std::atomic<std::uint64_t> recordEndAudioSample { 0 };
    std::atomic<bool> finished { false };
};

[[nodiscard]] juce::var runArrangeRecordingSelfTest(const juce::File& directory);

} // namespace riffra
