#pragma once

#include <JuceHeader.h>
#include "RecordingSession.h"
#include "PluginRack.h"

#include <atomic>
#include <memory>

namespace riffra {

class SafetyAudioCallback final : public juce::AudioIODeviceCallback {
public:
    SafetyAudioCallback() = default;
    ~SafetyAudioCallback() override;

    void setEmergencyMuted(bool shouldMute) noexcept;
    [[nodiscard]] bool isEmergencyMuted() const noexcept;
    void setMasterGainDb(float gainDb) noexcept;
    [[nodiscard]] float getMasterGainDb() const noexcept;
    [[nodiscard]] float getInputPeak() const noexcept;
    [[nodiscard]] float getOutputPeak() const noexcept;
    [[nodiscard]] std::uint64_t getInvalidSampleCount() const noexcept;
    [[nodiscard]] double getSampleRate() const noexcept;
    bool startRecording(const juce::File& directory, juce::String& error);
    bool stopRecording(juce::String& error);
    [[nodiscard]] juce::var recordingStatus() const;
    bool startPreview(juce::AudioBuffer<float>& buffer, int startSample, int endSample, float gain, bool loop, juce::String& error);
    void stopPreview() noexcept;
    [[nodiscard]] bool isPreviewing() const noexcept;
    void setPluginRack(PluginRack* rack) noexcept;


    void audioDeviceIOCallbackWithContext(
        const float* const* inputChannelData,
        int numInputChannels,
        float* const* outputChannelData,
        int numOutputChannels,
        int numSamples,
        const juce::AudioIODeviceCallbackContext& context) override;
    void audioDeviceAboutToStart(juce::AudioIODevice* device) override;
    void audioDeviceStopped() override;
    void audioDeviceError(const juce::String& errorMessage) override;

    [[nodiscard]] juce::String takeLastDeviceError();

private:
    static constexpr float kMinimumGainDb = -90.0f;
    void writeRecording(
        const float* const* inputChannelData,
        int numInputChannels,
        float* const* outputChannelData,
        int numOutputChannels,
        int numSamples) noexcept;
    void mixPreview(float* const* outputChannelData, int numOutputChannels, int numSamples) noexcept;


    static constexpr float kMaximumGainDb = 0.0f;
    static constexpr float kLimiterCeiling = 0.98f;
    static constexpr double kFadeInSeconds = 0.5;

    std::atomic<bool> emergencyMuted { true };
    std::atomic<float> targetGainLinear { juce::Decibels::decibelsToGain(-18.0f) };
    std::atomic<float> masterGainDb { -18.0f };
    std::atomic<float> inputPeak { 0.0f };
    std::atomic<float> outputPeak { 0.0f };
    std::atomic<std::uint64_t> invalidSamples { 0 };
    std::atomic<double> activeSampleRate { 0.0 };
    float currentGainLinear = 0.0f;
    float fadeStep = 0.0f;
    std::atomic<int> activeInputChannels { 0 };
    std::atomic<int> activeOutputChannels { 0 };
    juce::AudioBuffer<float> silenceBuffer;
    std::atomic<RecordingSession*> activeRecording { nullptr };
    std::atomic<unsigned int> recordingReaders { 0 };
    mutable juce::CriticalSection recordingLock;
    std::unique_ptr<RecordingSession> recording;
    mutable juce::CriticalSection previewLock;
    juce::AudioBuffer<float> previewBuffer;
    int previewStart = 0;
    int previewCursor = 0;
    int previewEnd = 0;
    float previewGain = 1.0f;
    bool previewLoop = false;
    bool previewActive = false;
    PluginRack* pluginRack = nullptr;

    juce::CriticalSection errorLock;
    juce::String lastDeviceError;
};

} // namespace riffra
