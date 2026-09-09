#pragma once

#include <JuceHeader.h>

#include <atomic>
#include <array>
#include <cstdint>
#include <utility>

#include "AudioMetrics.h"
#include "PreviewEngine.h"
#include "app/RecordingController.h"
#include "AudioSafetyDsp.h"

namespace riffra {

class TimelineEngine;

enum class MuteReason : std::uint32_t {
    UserEmergency = 1u << 0,
    EngineTransition = 1u << 1,
    DeviceFault = 1u << 2,
    FeedbackProtection = 1u << 3,
};

/// Owns the ordered realtime render and safety processing path.
class AudioRenderPipeline final {
public:
    explicit AudioRenderPipeline(TimelineEngine& timeline) noexcept;
    ~AudioRenderPipeline();

    AudioRenderPipeline(const AudioRenderPipeline&) = delete;
    AudioRenderPipeline& operator=(const AudioRenderPipeline&) = delete;

    // Control thread only. These methods update atomic control state consumed
    // by processBlock().
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
    [[nodiscard]] float getInputPeak() const noexcept { return audioMetrics.inputPeak(); }
    [[nodiscard]] float getOutputPeak() const noexcept { return audioMetrics.outputPeak(); }
    [[nodiscard]] std::uint64_t getInvalidSampleCount() const noexcept {
        return audioMetrics.invalidSampleCount();
    }
    [[nodiscard]] std::uint64_t getCallbackCount() const noexcept {
        return audioMetrics.callbackCount();
    }
    [[nodiscard]] std::uint64_t getAverageCallbackDurationUs() const noexcept {
        return audioMetrics.averageCallbackDurationUs();
    }
    [[nodiscard]] std::uint64_t getMaximumCallbackDurationUs() const noexcept {
        return audioMetrics.maximumCallbackDurationUs();
    }
    [[nodiscard]] std::uint64_t getCallbackOverruns() const noexcept {
        return audioMetrics.callbackOverruns();
    }
    [[nodiscard]] float getPreLimiterPeak() const noexcept {
        return audioMetrics.preLimiterPeak();
    }
    [[nodiscard]] float getLimiterGainReductionDb() const noexcept {
        return audioMetrics.limiterGainReductionDb();
    }
    [[nodiscard]] std::uint64_t getHardClipSamples() const noexcept {
        return audioMetrics.hardClipSamples();
    }
    [[nodiscard]] bool isFeedbackSuspected() const noexcept {
        return feedbackSuspected.load(std::memory_order_acquire);
    }
    [[nodiscard]] bool isPreviewing() const noexcept;
    [[nodiscard]] juce::var recordingStatus() const { return recordingController.status(); }
    [[nodiscard]] double getSampleRate() const noexcept;

    [[nodiscard]] AudioMetrics& metrics() noexcept { return audioMetrics; }
    [[nodiscard]] const AudioMetrics& metrics() const noexcept { return audioMetrics; }
    [[nodiscard]] PreviewEngine& preview() noexcept { return previewEngine; }
    [[nodiscard]] RecordingController& recording() noexcept { return recordingController; }

    // Control-thread forwarding kept at the pipeline boundary while command
    // dispatch is moved in a later phase.
    bool startArrangeRecording(const juce::File& directory, TimelineEngine& timeline,
                               juce::String& error) {
        juce::ignoreUnused(timeline);
        return recordingController.start(directory, error);
    }
    void setRecordingFinalizationDispatcher(RecordingController::FinalizationDispatcher dispatcher) {
        recordingController.setFinalizationDispatcher(std::move(dispatcher));
    }
    bool stopArrangeRecording(TimelineEngine& timeline, juce::String& error) {
        juce::ignoreUnused(timeline);
        return recordingController.stop(error);
    }
    std::unique_ptr<ArrangeRecordingSession> takeFinalizedRecording() noexcept {
        return recordingController.takePendingFinalization();
    }
    void completeArrangeRecordingProcessing(const juce::var& status, const juce::String& error) {
        recordingController.completeProcessing(status, error);
    }
    bool cancelArrangeRecording(TimelineEngine& timeline, juce::String& error) {
        juce::ignoreUnused(timeline);
        return recordingController.cancel(error);
    }
    bool startPreview(juce::AudioBuffer<float>& buffer, int startSample, int endSample, float gain,
                      bool loop, juce::String& error, int voiceKey = -1) {
        return previewEngine.startPreview(buffer, startSample, endSample, gain, loop, error,
                                          voiceKey);
    }
    void stopPreview() noexcept { previewEngine.stopPreview(); }
    void stopPreviewForKey(int voiceKey) noexcept { previewEngine.stopPreviewForKey(voiceKey); }
    bool switchPreviewBuffer(int voiceKey, const juce::AudioBuffer<float>& buffer,
                             juce::String& error) {
        return previewEngine.switchPreviewBuffer(voiceKey, buffer, error);
    }
    void startSynthNote(int note, float velocity) noexcept {
        previewEngine.startSynthNote(note, velocity);
    }
    void stopSynthNote(int note) noexcept { previewEngine.stopSynthNote(note); }
    void allNotesOff() noexcept { previewEngine.allNotesOff(); }

    // Audio thread only. The processing order is the contract of the engine.
    void processBlock(const float* const* inputChannelData, int numInputChannels,
                      float* const* outputChannelData, int numOutputChannels, int numSamples,
                      const juce::AudioIODeviceCallbackContext& context) noexcept;

    // Main / JUCE message thread only.
    void prepare(juce::AudioIODevice* device);
    void deviceStopped() noexcept;

private:
    static constexpr float kMinimumGainDb = -90.0f;
    static constexpr float kMaximumGainDb = 0.0f;
    static constexpr float kLimiterCeiling = 0.98f;
    static constexpr double kFadeInSeconds = 0.05;

    static std::uint32_t muteReasonBit(MuteReason reason) noexcept;
    void setMuteReason(MuteReason reason, bool active) noexcept;
    void silenceAndCommit(float* const* outputChannelData, int numOutputChannels, int numSamples,
                          float rawInputPeak) noexcept;

    TimelineEngine& timelineEngine;
    AudioMetrics audioMetrics;
    PreviewEngine previewEngine;
    RecordingController recordingController;

    std::atomic<std::uint32_t> muteReasons{0};
    std::atomic<float> targetGainLinear{1.0f};
    std::atomic<float> masterGainDb{0.0f};
    std::atomic<int> inputChannel{0};
    std::atomic<bool> panicRequested{false};
    std::atomic<bool> resetGainOnNextCallback{true};
    std::atomic<bool> feedbackSuspected{false};
    std::atomic<bool> deviceTransitionActive{false};
    std::atomic<double> activeSampleRate{0.0};
    float currentGainLinear = 0.0f;
    float fadeStep = 0.0f;
    DCBlocker dcBlocker;
    FeedbackDetector feedbackDetector;
    juce::dsp::Limiter<float> limiter;
    std::array<float*, 64> limiterChannels{};
    bool limiterPrepared = false;
};

}  // namespace riffra
