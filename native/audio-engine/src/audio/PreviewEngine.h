#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <unordered_map>
#include <vector>

namespace riffra {

struct InstrumentPreviewSpec;
class InstrumentPreviewSession;

/// Owns preview sample voices and the fallback MIDI synthesizer.
class PreviewEngine final {
public:
    PreviewEngine();
    ~PreviewEngine();

    // Non-audio callback threads only. Control updates publish immutable
    // snapshots; the audio callback keeps using its previous snapshot until a
    // callback boundary.
    bool startPreview(juce::AudioBuffer<float>& buffer, int startSample, int endSample, float gain,
                      bool loop, juce::String& error, int voiceKey = -1);
    bool startBuiltInPreview(const juce::String& definitionJson,
                             const juce::String& definitionBaseDir, InstrumentPreviewSpec spec,
                             double sampleRate, int blockSize, juce::String& error);
    void stopBuiltInPreview() noexcept;
    void stopPreview() noexcept;
    void stopPreviewForKey(int voiceKey) noexcept;
    bool switchPreviewBuffer(int voiceKey, const juce::AudioBuffer<float>& buffer,
                             juce::String& error);
    void startSynthNote(int note, float velocity) noexcept;
    void stopSynthNote(int note) noexcept;
    void allNotesOff() noexcept;
    [[nodiscard]] bool isPreviewing() const noexcept;
    [[nodiscard]] bool isBuiltInPreviewing() const noexcept;

    // Device lifecycle/control side only.
    void prepare() noexcept;

    // Audio thread only. It always renders the last committed state; a
    // control update never causes a whole callback to be skipped.
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
        PreviewAudioGuard(const PreviewAudioGuard&) = delete;
        PreviewAudioGuard& operator=(const PreviewAudioGuard&) = delete;

    private:
        PreviewEngine& owner;
        std::uint32_t generation = 0;
        bool entered = false;
    };

    static constexpr int kSineLUTSize = 2048;
    static constexpr double kTwoPi = 6.2831853071795864769;
    static constexpr std::size_t kAudioReaderGenerationCount = 2;
    static constexpr std::size_t kPreviewVoiceCount = 8;
    static constexpr std::size_t kSynthVoiceCount = 16;

    enum class VoiceState { inactive, fadingIn, playing, fadingOut };

    struct PreviewVoiceConfig final {
        const juce::AudioBuffer<float>* buffer = nullptr;
        int key = -1;
        int start = 0;
        int cursor = 0;
        int end = 0;
        float gain = 1.0f;
        bool loop = false;
        bool active = false;
        std::uint64_t revision = 0;
    };

    struct PreviewState final {
        std::array<PreviewVoiceConfig, kPreviewVoiceCount> voices;
        InstrumentPreviewSession* builtInSession = nullptr;
        juce::AudioBuffer<float>* builtInBuffer = nullptr;
    };

    struct PreviewVoiceRuntime final {
        VoiceState state = VoiceState::inactive;
        PreviewState* sourceState = nullptr;
        PreviewState* pendingState = nullptr;
        const juce::AudioBuffer<float>* buffer = nullptr;
        int key = -1;
        int start = 0;
        int cursor = 0;
        int end = 0;
        float gain = 1.0f;
        bool loop = false;
        float fadeGain = 0.0f;
        float fadeStep = 0.0f;
        float lastSample[2] = {0.0f, 0.0f};
    };

    struct SynthControlState final {
        std::atomic<int> note{-1};
        std::atomic<float> frequency{0.0f};
        std::atomic<float> targetLevel{0.0f};
        std::atomic<std::uint64_t> revision{0};
        std::atomic<std::uint64_t> audioFinishedRevision{0};
        std::atomic<bool> active{false};
        std::atomic<bool> releasing{false};
    };

    struct SynthVoice final {
        int note = -1;
        float frequency = 0.0f;
        float phase = 0.0f;
        float level = 0.0f;
        float targetLevel = 0.0f;
        bool active = false;
        bool releasing = false;
        std::uint64_t controlRevision = 0;
    };

    static float lookupSine(float phase) noexcept;
    void publishState(std::unique_ptr<PreviewState> next);
    void retireFinishedBuiltInState();
    void cleanupDeferredState() noexcept;
    void applyPreviewState(PreviewState& state, double sampleRate) noexcept;
    void configureVoice(PreviewVoiceRuntime& voice, PreviewState* sourceState,
                        const PreviewVoiceConfig& config, double sampleRate) noexcept;
    void beginVoiceFadeOut(PreviewVoiceRuntime& voice, double sampleRate) noexcept;
    void finishVoiceFade(PreviewVoiceRuntime& voice, std::size_t index, double sampleRate) noexcept;
    void mixPreview(float* const* outputChannelData, int numOutputChannels, int numSamples,
                    double sampleRate) noexcept;
    void mixBuiltInPreview(float* const* outputChannelData, int numOutputChannels, int numSamples,
                           double sampleRate) noexcept;
    void syncSynthVoices() noexcept;
    void mixSynth(float* const* outputChannelData, int numOutputChannels, int numSamples,
                  double sampleRate) noexcept;

    std::atomic_flag previewBusy = ATOMIC_FLAG_INIT;
    // A control update switches generations before reclaiming. Readers that
    // started before the switch drain from the old generation while new
    // readers continue on the new one.
    std::array<std::atomic<std::uint32_t>, kAudioReaderGenerationCount> audioReaderCounts{};
    std::atomic<std::uint32_t> audioReaderGeneration{0};
    std::uint32_t deferredCleanupGeneration = 0;
    bool deferredCleanupPending = false;
    std::atomic<PreviewState*> pendingPreviewState{nullptr};
    std::atomic<PreviewState*> audioPreviewState{nullptr};
    std::array<std::atomic<PreviewState*>, kPreviewVoiceCount> audioVoiceStates{};
    std::array<std::atomic<PreviewState*>, kPreviewVoiceCount> audioVoicePendingStates{};
    std::array<std::atomic<int>, kPreviewVoiceCount> audioVoiceCursors{};
    std::vector<std::unique_ptr<PreviewState>> previewStates;
    std::vector<std::unique_ptr<juce::AudioBuffer<float>>> previewBuffers;
    std::vector<std::unique_ptr<InstrumentPreviewSession>> builtInSessions;
    std::vector<std::unique_ptr<juce::AudioBuffer<float>>> builtInBuffers;
    // A pointer is first marked with the active reader generation and can only
    // be destroyed after that generation becomes the drained generation.
    std::unordered_map<const PreviewState*, std::uint32_t> retiredPreviewStates;
    std::unordered_map<const juce::AudioBuffer<float>*, std::uint32_t> retiredPreviewBuffers;
    std::unordered_map<const InstrumentPreviewSession*, std::uint32_t> retiredBuiltInSessions;
    std::unordered_map<const juce::AudioBuffer<float>*, std::uint32_t> retiredBuiltInBuffers;

    std::array<PreviewVoiceRuntime, kPreviewVoiceCount> previewVoices;
    std::uint64_t previewSequence = 0;

    std::atomic<InstrumentPreviewSession*> audioBuiltInSession{nullptr};
    std::atomic<InstrumentPreviewSession*> audioBuiltInPendingSession{nullptr};
    std::atomic<juce::AudioBuffer<float>*> audioBuiltInBuffer{nullptr};
    std::atomic<juce::AudioBuffer<float>*> audioBuiltInPendingBuffer{nullptr};
    std::atomic<bool> builtInStopRequested{false};
    InstrumentPreviewSession* pendingBuiltInSession = nullptr;
    enum class BuiltInState { inactive, fadingIn, playing, fadingOut };
    BuiltInState builtInState = BuiltInState::inactive;
    float builtInGain = 0.0f;
    float builtInFadeStep = 0.0f;
    float builtInFadeAnchor[2] = {0.0f, 0.0f};
    float builtInLastOutput[2] = {0.0f, 0.0f};

    std::array<SynthControlState, kSynthVoiceCount> synthControl;
    std::array<SynthVoice, kSynthVoiceCount> synthVoices;
    std::atomic<bool> synthPanicRequested{false};
};

}  // namespace riffra
