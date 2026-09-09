#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <functional>
#include <memory>

#include "ArrangeRecordingSession.h"
#include "AudioSafetyDsp.h"
#include "TimelineEngine.h"

namespace riffra {

enum class MuteReason : std::uint32_t {
    UserEmergency = 1u << 0,
    EngineTransition = 1u << 1,
    DeviceFault = 1u << 2,
    FeedbackProtection = 1u << 3,
};

class SafetyAudioCallback final : public juce::AudioIODeviceCallback {
public:
    using RecordingFinalizationDispatcher =
        std::function<void(std::unique_ptr<ArrangeRecordingSession>)>;

    SafetyAudioCallback() = default;
    ~SafetyAudioCallback() override;

    // Control thread only. The audio callback reads the resulting atomics.
    void setUserEmergencyMute(bool shouldMute) noexcept;
    void setEngineTransitionMute(bool active) noexcept;
    void setFeedbackProtection(bool active) noexcept;
    [[nodiscard]] std::uint32_t getMuteReasons() const noexcept;
    [[nodiscard]] bool isMuted() const noexcept;
    [[nodiscard]] bool hasMuteReason(MuteReason reason) const noexcept;
    void setDeviceFaulted(bool faulted) noexcept;
    [[nodiscard]] bool isDeviceFaulted() const noexcept;
    void setDeviceTransitionActive(bool active) noexcept;
    [[nodiscard]] bool isDeviceTransitionActive() const noexcept;
    void setMasterGainDb(float gainDb) noexcept;
    void setInputChannel(int channel) noexcept;
    [[nodiscard]] int getInputChannel() const noexcept;
    [[nodiscard]] float getMasterGainDb() const noexcept;
    [[nodiscard]] float getInputPeak() const noexcept;
    [[nodiscard]] float getOutputPeak() const noexcept;
    [[nodiscard]] std::uint64_t getInvalidSampleCount() const noexcept;
    [[nodiscard]] std::uint64_t getCallbackCount() const noexcept;
    [[nodiscard]] std::uint64_t getAverageCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t getMaximumCallbackDurationUs() const noexcept;
    [[nodiscard]] std::uint64_t getCallbackOverruns() const noexcept;
    [[nodiscard]] float getPreLimiterPeak() const noexcept;
    [[nodiscard]] float getLimiterGainReductionDb() const noexcept;
    [[nodiscard]] std::uint64_t getHardClipSamples() const noexcept;
    [[nodiscard]] bool isFeedbackSuspected() const noexcept;
    [[nodiscard]] double getSampleRate() const noexcept;
    bool startArrangeRecording(const juce::File& directory, TimelineEngine& timeline,
                               juce::String& error);
    void setRecordingFinalizationDispatcher(RecordingFinalizationDispatcher dispatcher);
    bool stopArrangeRecording(TimelineEngine& timeline, juce::String& error);
    std::unique_ptr<ArrangeRecordingSession> takeFinalizedRecording() noexcept;
    void completeArrangeRecordingProcessing(const juce::var& status, const juce::String& error);
    bool cancelArrangeRecording(TimelineEngine& timeline, juce::String& error);
    [[nodiscard]] juce::var recordingStatus() const;
    bool startPreview(juce::AudioBuffer<float>& buffer, int startSample, int endSample, float gain,
                      bool loop, juce::String& error, int voiceKey = -1);
    void stopPreview() noexcept;
    void stopPreviewForKey(int voiceKey) noexcept;
    bool switchPreviewBuffer(int voiceKey, const juce::AudioBuffer<float>& buffer,
                             juce::String& error);
    void startSynthNote(int note, float velocity) noexcept;
    void stopSynthNote(int note) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isPreviewing() const noexcept;
    void setTimelineEngine(TimelineEngine* engine) noexcept;

    // Audio thread only. No allocation, blocking wait, device lifecycle, or
    // plugin lifecycle work may be introduced on this path.
    void audioDeviceIOCallbackWithContext(
        const float* const* inputChannelData, int numInputChannels, float* const* outputChannelData,
        int numOutputChannels, int numSamples,
        const juce::AudioIODeviceCallbackContext& context) override;
    // Main / JUCE message thread only.
    void audioDeviceAboutToStart(juce::AudioIODevice* device) override;
    void audioDeviceStopped() override;
    void audioDeviceError(const juce::String& errorMessage) override;

    [[nodiscard]] juce::String takeLastDeviceError();

private:
    static constexpr float kMinimumGainDb = -90.0f;
    static void holdPeak(std::atomic<float>& peak, float value) noexcept;
    void mixPreview(float* const* outputChannelData, int numOutputChannels,
                    int numSamples) noexcept;
    void mixSynth(float* const* outputChannelData, int numOutputChannels, int numSamples) noexcept;

    /// Shared epilogue for silenced paths. Clears the output bus, holds the
    /// input peak, and zeroes the output peak.
    void silenceAndCommit(float* const* outputChannelData, int numOutputChannels, int numSamples,
                          float rawInputPeak) noexcept;

    class PreviewControlGuard final {
    public:
        explicit PreviewControlGuard(SafetyAudioCallback& owner) noexcept;
        ~PreviewControlGuard();

        PreviewControlGuard(const PreviewControlGuard&) = delete;
        PreviewControlGuard& operator=(const PreviewControlGuard&) = delete;

    private:
        SafetyAudioCallback& owner;
    };

    class PreviewAudioGuard final {
    public:
        explicit PreviewAudioGuard(SafetyAudioCallback& owner) noexcept;
        ~PreviewAudioGuard();
        [[nodiscard]] bool acquired() const noexcept { return ownsLock; }

        PreviewAudioGuard(const PreviewAudioGuard&) = delete;
        PreviewAudioGuard& operator=(const PreviewAudioGuard&) = delete;

    private:
        SafetyAudioCallback& owner;
        bool ownsLock = false;
    };

    /// Sine lookup table size. Power-of-two keeps the index wrap cheap; the
    /// stored table has one extra element duplicating index 0 so linear
    /// interpolation can read `lut[i0 + 1]` without a separate wrap branch.
    static constexpr int kSineLUTSize = 2048;
    static constexpr double kTwoPi = 6.2831853071795864769;
    /// Looks up `sin(phase)` from the precomputed table with linear
    /// interpolation. `phase` must already be wrapped into `[0, 2π)`. Replaces
    /// per-sample `std::sin` in the realtime synth path so libm never runs on
    /// the audio thread.
    static float lookupSine(float phase) noexcept;

    static constexpr float kMaximumGainDb = 0.0f;
    static constexpr float kLimiterCeiling = 0.98f;
    static constexpr double kFadeInSeconds = 0.05;

    void setMuteReason(MuteReason reason, bool active) noexcept;

    // Thread-safe state shared by the control and audio threads.
    std::atomic<std::uint32_t> muteReasons{0};
    std::atomic<float> targetGainLinear{1.0f};
    std::atomic<float> masterGainDb{0.0f};
    std::atomic<int> inputChannel{0};
    mutable std::atomic<float> inputPeak{0.0f};
    mutable std::atomic<float> outputPeak{0.0f};
    std::atomic<std::uint64_t> invalidSamples{0};
    std::atomic<std::uint64_t> callbackCount{0};
    std::atomic<std::uint64_t> callbackDurationUs{0};
    std::atomic<std::uint64_t> maximumCallbackDurationUs{0};
    std::atomic<std::uint64_t> callbackOverruns{0};
    mutable std::atomic<float> preLimiterPeak{0.0f};
    mutable std::atomic<float> limiterGainReductionDb{0.0f};
    std::atomic<std::uint64_t> hardClipSamples{0};
    std::atomic<bool> panicRequested{false};
    std::atomic<bool> synthPanicRequested{false};
    std::atomic<bool> resetGainOnNextCallback{true};
    std::atomic<bool> feedbackSuspected{false};
    std::atomic<bool> deviceTransitionActive{false};
    std::atomic<double> activeSampleRate{0.0};
    float currentGainLinear = 0.0f;
    float fadeStep = 0.0f;
    mutable juce::CriticalSection recordingLock;
    std::unique_ptr<ArrangeRecordingSession> arrangeRecording;
    std::unique_ptr<ArrangeRecordingSession> pendingFinalization;
    RecordingFinalizationDispatcher recordingFinalizationDispatcher;
    juce::var recordingFinalizationStatus;
    bool recordingProcessing = false;
    std::atomic<bool> arrangeRecordingCancelled{false};
    std::atomic_flag previewBusy = ATOMIC_FLAG_INIT;
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
        /// Cached frequency (Hz) computed from `note` at voice start so the
        /// per-sample path never calls libm on the realtime thread.
        float frequency = 0.0f;
        float phase = 0.0f;
        float level = 0.0f;
        float targetLevel = 0.0f;
        bool active = false;
        bool releasing = false;
    };
    static constexpr std::size_t kSynthVoiceCount = 16;
    std::array<SynthVoice, kSynthVoiceCount> synthVoices;
    TimelineEngine* timelineEngine = nullptr;

    juce::CriticalSection errorLock;
    juce::String lastDeviceError;
    DCBlocker dcBlocker;
    FeedbackDetector feedbackDetector;
    juce::dsp::Limiter<float> limiter;
    std::array<float*, 64> limiterChannels{};
    bool limiterPrepared = false;

    void recordCallbackDuration(std::chrono::steady_clock::time_point started,
                                int numSamples) noexcept;
};

}  // namespace riffra
