#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstdint>

namespace riffra {

/// Owns preview sample voices and the fallback MIDI synthesizer.
class PreviewEngine final {
public:
    // Control thread only. Audio playback observes the state through the
    // non-blocking audio guard.
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

    // Main / JUCE message thread only.
    void prepare() noexcept;

    // Audio thread only. Returns false when a control-thread update currently
    // owns the preview state.
    bool tryMix(float* const* outputChannelData, int numOutputChannels, int numSamples,
                double sampleRate) noexcept;
    void requestSynthPanic() noexcept;

private:
    class PreviewControlGuard final {
    public:
        explicit PreviewControlGuard(PreviewEngine& owner) noexcept;
        ~PreviewControlGuard();
        PreviewControlGuard(const PreviewControlGuard&) = delete;
        PreviewControlGuard& operator=(const PreviewControlGuard&) = delete;

    private:
        PreviewEngine& owner;
    };

    class PreviewAudioGuard final {
    public:
        explicit PreviewAudioGuard(PreviewEngine& owner) noexcept;
        ~PreviewAudioGuard();
        [[nodiscard]] bool acquired() const noexcept { return ownsLock; }
        PreviewAudioGuard(const PreviewAudioGuard&) = delete;
        PreviewAudioGuard& operator=(const PreviewAudioGuard&) = delete;

    private:
        PreviewEngine& owner;
        bool ownsLock = false;
    };

    static constexpr int kSineLUTSize = 2048;
    static constexpr double kTwoPi = 6.2831853071795864769;
    static constexpr std::size_t kPreviewVoiceCount = 8;
    static constexpr std::size_t kSynthVoiceCount = 16;

    static float lookupSine(float phase) noexcept;
    void mixPreview(float* const* outputChannelData, int numOutputChannels,
                    int numSamples) noexcept;
    void mixSynth(float* const* outputChannelData, int numOutputChannels, int numSamples,
                  double sampleRate) noexcept;

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

    struct SynthVoice {
        int note = -1;
        float frequency = 0.0f;
        float phase = 0.0f;
        float level = 0.0f;
        float targetLevel = 0.0f;
        bool active = false;
        bool releasing = false;
    };

    std::atomic_flag previewBusy = ATOMIC_FLAG_INIT;
    std::array<PreviewVoice, kPreviewVoiceCount> previewVoices;
    std::uint64_t previewSequence = 0;
    std::array<SynthVoice, kSynthVoiceCount> synthVoices;
    std::atomic<bool> synthPanicRequested{false};
};

}  // namespace riffra
