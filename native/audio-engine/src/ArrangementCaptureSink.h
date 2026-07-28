#pragma once

#include <JuceHeader.h>
#include <cstdint>
#include <vector>

namespace riffra {

class ArrangementCaptureSink {
public:
    virtual ~ArrangementCaptureSink() = default;
    virtual bool beginAudioTrackCapture(
        const juce::String& trackId,
        std::uint64_t audioClockStartSample,
        std::uint64_t timelineStartSample) noexcept = 0;
    virtual void writeAudioTrack(
        const juce::String& trackId,
        const float* raw,
        int rawSampleCount,
        const float* const* processed,
        int processedSampleCount) noexcept = 0;
    virtual bool endAudioTrackCapture(
        const juce::String& trackId,
        std::uint64_t audioClockEndSample,
        std::uint64_t timelineEndSample) noexcept = 0;
    virtual bool completeAudioTrackTail(const juce::String& trackId) noexcept = 0;
    virtual void markLoopBoundary(std::uint64_t audioSample) noexcept = 0;
    virtual void writeMidiTrack(
        const juce::String& trackId,
        const juce::String& sourceDeviceId,
        const juce::MidiMessage& message,
        std::uint64_t audioSample) noexcept = 0;
    virtual void setCaptureRange(
        std::uint64_t startAudioSample,
        std::uint64_t endAudioSample,
        std::uint64_t startTimelineSample,
        std::uint64_t endTimelineSample) noexcept = 0;

    /// Flush the raw writer and prepare for offline reading.
    /// Returns the path to the raw WAV file, or empty on failure.
    virtual juce::File prepareRawForReading(const juce::String& trackId) noexcept {
        juce::ignoreUnused(trackId);
        return {};
    }

    /// Returns the raw file segment boundaries [start, end) for a track.
    virtual std::vector<std::pair<std::uint64_t, std::uint64_t>>
    getRawSegmentRanges(const juce::String& trackId) noexcept {
        juce::ignoreUnused(trackId);
        return {};
    }
};

} // namespace riffra
