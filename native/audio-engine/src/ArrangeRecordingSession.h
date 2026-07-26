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

    void beginAudioTrackCapture(
        const juce::String& trackId,
        std::uint64_t audioClockStartSample,
        std::uint64_t timelineStartSample) noexcept override;
    void writeAudioTrack(
        const juce::String& trackId,
        const float* raw,
        int rawSampleCount,
        const float* const* processed,
        int processedSampleCount) noexcept override;
    void endAudioTrackCapture(
        const juce::String& trackId,
        std::uint64_t audioClockEndSample,
        std::uint64_t timelineEndSample) noexcept override;
    void markLoopBoundary(std::uint64_t audioSample) noexcept override;
    void writeMidiTrack(
        const juce::String& trackId,
        const juce::String& sourceDeviceId,
        const juce::MidiMessage& message,
        std::uint64_t audioSample) noexcept override;
    void setCaptureRange(
        std::uint64_t startAudioSample,
        std::uint64_t endAudioSample,
        std::uint64_t startTimelineSample,
        std::uint64_t endTimelineSample) noexcept override;
    bool finish(juce::String& error);
    bool cancel(juce::String& error);
    [[nodiscard]] juce::var status() const;

private:
    static constexpr std::size_t kMaximumLoopBoundaries = 4096;
    static constexpr std::size_t kMaximumCaptureSegments = 4096;

    struct CaptureSegment final {
        std::uint64_t audioClockStartSample = 0;
        std::uint64_t audioClockEndSample = 0;
        std::uint64_t timelineStartSample = 0;
        std::uint64_t timelineEndSample = 0;
        std::uint64_t fileStartSample = 0;
        std::uint64_t fileEndSample = 0;
    };

    struct TrackWriter final {
        struct VariantCaptureSegment final {
            std::uint64_t audioClockStartSample = 0;
            std::uint64_t audioClockEndSample = 0;
            std::uint64_t timelineStartSample = 0;
            std::uint64_t timelineEndSample = 0;
            std::uint64_t rawFileStartSample = 0;
            std::uint64_t rawFileEndSample = 0;
            std::uint64_t processedFileStartSample = 0;
            std::uint64_t processedFileEndSample = 0;
            std::uint64_t processedTailEndSample = 0;
        };
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
        int pluginTailSamples = 0;
        std::unique_ptr<RecordingSession> audio;
        std::vector<MidiEvent> midiEvents;
        // Allocate this before recording begins; audio callbacks never grow
        // the vector and therefore never allocate.
        static constexpr std::size_t kMaximumTrackCaptureSegments = kMaximumCaptureSegments;
        std::vector<VariantCaptureSegment> captureSegments;
        std::size_t captureSegmentCount = 0;
        bool captureActive = false;
    };

    ArrangeRecordingSession(juce::File directory, double sampleRate);
    bool initialise(const juce::var& configuration, juce::String& error);
    bool writeManifest(const juce::String& state, juce::String& error) const;

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
    std::array<CaptureSegment, kMaximumCaptureSegments> captureSegments {};
    std::atomic<std::size_t> captureSegmentCount { 0 };
    std::uint64_t capturedFileSamples = 0;
    std::atomic<std::uint64_t> recordStartAudioSample {
        std::numeric_limits<std::uint64_t>::max()
    };
    std::atomic<std::uint64_t> recordEndAudioSample { 0 };
    std::atomic<std::uint64_t> recordStartTimelineSample {
        std::numeric_limits<std::uint64_t>::max()
    };
    std::atomic<std::uint64_t> recordEndTimelineSample { 0 };
    std::atomic<bool> finished { false };
};

[[nodiscard]] juce::var runArrangeRecordingSelfTest(const juce::File& directory);

} // namespace riffra
