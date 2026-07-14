#pragma once

#include <JuceHeader.h>
#include "AudioSafetyDsp.h"
#include "RecordingSession.h"
#include "PluginRack.h"

#include <atomic>
#include <array>
#include <memory>

namespace riffra {

class SafetyAudioCallback final : public juce::AudioIODeviceCallback {
public:
    SafetyAudioCallback() = default;
    ~SafetyAudioCallback() override;

    void setEmergencyMuted(bool shouldMute) noexcept;
    [[nodiscard]] bool isEmergencyMuted() const noexcept;
    void setDeviceFaulted(bool faulted) noexcept;
    [[nodiscard]] bool isDeviceFaulted() const noexcept;
    void setMasterGainDb(float gainDb) noexcept;
    [[nodiscard]] float getMasterGainDb() const noexcept;
    [[nodiscard]] float getInputPeak() const noexcept;
    [[nodiscard]] float getOutputPeak() const noexcept;
    [[nodiscard]] std::uint64_t getInvalidSampleCount() const noexcept;
    [[nodiscard]] bool isFeedbackSuspected() const noexcept;
    [[nodiscard]] double getSampleRate() const noexcept;
    bool startRecording(const juce::File& directory, juce::String& error);
    bool stopRecording(juce::String& error);
    [[nodiscard]] juce::var recordingStatus() const;
    bool startPreview(juce::AudioBuffer<float>& buffer, int startSample, int endSample, float gain, bool loop, juce::String& error, int voiceKey = -1);
    void stopPreview() noexcept;
    void stopPreviewForKey(int voiceKey) noexcept;
    void startSynthNote(int note, float velocity) noexcept;
    void stopSynthNote(int note) noexcept;
    void allNotesOff() noexcept;
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
    void mixSynth(float* const* outputChannelData, int numOutputChannels, int numSamples) noexcept;


    static constexpr float kMaximumGainDb = 0.0f;
    static constexpr float kLimiterCeiling = 0.98f;
    static constexpr double kFadeInSeconds = 0.5;

    std::atomic<bool> emergencyMuted { true };
    std::atomic<bool> deviceFaulted { false };
    std::atomic<float> targetGainLinear { juce::Decibels::decibelsToGain(-18.0f) };
    std::atomic<float> masterGainDb { -18.0f };
    std::atomic<float> inputPeak { 0.0f };
    std::atomic<float> outputPeak { 0.0f };
    std::atomic<std::uint64_t> invalidSamples { 0 };
    std::atomic<bool> feedbackSuspected { false };
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
    struct PreviewVoice {
        juce::AudioBuffer<float> buffer;
        int key = -1;
        int start = 0;
        int cursor = 0;
        int end = 0;
        float gain = 1.0f;
        bool loop = false;
        bool active = false;
        std::uint64_t sequence = 0;
    };
    static constexpr std::size_t kPreviewVoiceCount = 8;
    std::array<PreviewVoice, kPreviewVoiceCount> previewVoices;
    std::uint64_t previewSequence = 0;
    struct SynthVoice {
        int note = -1;
        float phase = 0.0f;
        float level = 0.0f;
        float targetLevel = 0.0f;
        bool active = false;
        bool releasing = false;
    };
    static constexpr std::size_t kSynthVoiceCount = 16;
    std::array<SynthVoice, kSynthVoiceCount> synthVoices;
    PluginRack* pluginRack = nullptr;

    juce::CriticalSection errorLock;
    juce::String lastDeviceError;
    DCBlocker dcBlocker;
    FeedbackDetector feedbackDetector;
};

} // namespace riffra
