#pragma once

#include <JuceHeader.h>
#include <cstdint>

namespace riffra {

class ArrangementCaptureSink {
public:
    virtual ~ArrangementCaptureSink() = default;
    virtual void writeAudioTrack(
        const juce::String& trackId,
        const float* raw,
        const float* const* processed,
        int sampleCount) noexcept = 0;
    virtual void markLoopBoundary(std::uint64_t audioSample) noexcept = 0;
    virtual void writeMidiTrack(
        const juce::String& trackId,
        const juce::String& sourceDeviceId,
        const juce::MidiMessage& message,
        std::uint64_t audioSample) noexcept = 0;
    virtual void setCaptureRange(
        std::uint64_t startAudioSample,
        std::uint64_t endAudioSample) noexcept = 0;
};

} // namespace riffra
