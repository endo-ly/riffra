#pragma once

#include <JuceHeader.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <mutex>
#include <vector>

#include "concurrency/RealtimeCommandQueue.h"
#include "concurrency/RetireQueue.h"

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
    bool startInstrumentPreview(const juce::String& definitionJson,
                                const juce::String& definitionBaseDir, InstrumentPreviewSpec spec,
                                double sampleRate, int blockSize, juce::String& error);
    bool stopInstrumentPreview(juce::String* error = nullptr);
    bool stopPreview(juce::String* error = nullptr);
    bool stopPreviewForKey(int voiceKey, juce::String* error = nullptr);
    bool switchPreviewBuffer(int voiceKey, const juce::AudioBuffer<float>& buffer,
                             juce::String& error);
    bool startSynthNote(int note, float velocity) noexcept;
    bool stopSynthNote(int note) noexcept;
    bool allNotesOff() noexcept;
    [[nodiscard]] bool isPreviewing() const noexcept;
    [[nodiscard]] bool isInstrumentPreviewing() const noexcept;

    // Device lifecycle/control side only.
    void prepare() noexcept;

    // Audio thread only. It always renders the last committed state; a
    // control update never causes a whole callback to be skipped.
    bool tryMix(float* const* outputChannelData, int numOutputChannels, int numSamples,
                double sampleRate) noexcept;
    bool requestSynthPanic() noexcept;

private:
    friend class PreviewEngineTestPeer;
    friend class AudioRenderPipeline;
    static constexpr int kSineLUTSize = 2048;
    static constexpr double kTwoPi = 6.2831853071795864769;
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
        InstrumentPreviewSession* instrumentSession = nullptr;
        juce::AudioBuffer<float>* instrumentBuffer = nullptr;
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

    struct SynthVoice final {
        int note = -1;
        float frequency = 0.0f;
        float phase = 0.0f;
        float level = 0.0f;
        float targetLevel = 0.0f;
        bool active = false;
        bool releasing = false;
    };

    static float lookupSine(float phase) noexcept;
    struct PreviewCommand final {
        enum class Kind {
            publishPreviewState,
            publishVoice,
            stopVoice,
            startInstrumentPreview,
            stopInstrumentPreview,
            synthNoteOn,
            synthNoteOff,
            synthPanic,
            stopAll
        };
        Kind kind = Kind::publishPreviewState;
        PreviewState* state = nullptr;
        std::size_t index = 0;
        int key = -1;
        int note = -1;
        float velocity = 0.0f;
        InstrumentPreviewSession* session = nullptr;
        juce::AudioBuffer<float>* buffer = nullptr;
    };
    bool publishState(std::unique_ptr<PreviewState> next, PreviewCommand::Kind kind,
                      juce::String& error, std::size_t index = 0, int key = -1);
    void reclaimRetiredState();
    void retireUnusedStates() noexcept;
    void applyCommand(const PreviewCommand& command, double sampleRate) noexcept;
    void applyPreviewState(PreviewState& state, double sampleRate,
                           const PreviewCommand* command = nullptr) noexcept;
    void configureVoice(PreviewVoiceRuntime& voice, PreviewState* sourceState,
                        const PreviewVoiceConfig& config, double sampleRate) noexcept;
    void beginVoiceFadeOut(PreviewVoiceRuntime& voice, double sampleRate) noexcept;
    void finishVoiceFade(PreviewVoiceRuntime& voice, std::size_t index, double sampleRate) noexcept;
    void mixPreview(float* const* outputChannelData, int numOutputChannels, int numSamples,
                    double sampleRate) noexcept;
    void mixInstrumentPreview(float* const* outputChannelData, int numOutputChannels,
                              int numSamples, double sampleRate) noexcept;
    void mixSynth(float* const* outputChannelData, int numOutputChannels, int numSamples,
                  double sampleRate) noexcept;

    std::mutex controlMutex;
    RealtimeCommandQueue<PreviewCommand, 256> commands;
    RetireQueue<PreviewState, 512> retiredStates;
    PreviewState controlState{};
    PreviewState* audioPreviewState = nullptr;
    std::array<PreviewState*, 512> audioStates{};
    std::size_t audioStateCount = 0;
    std::array<std::atomic<int>, kPreviewVoiceCount> audioVoiceCursors{};
    std::vector<std::unique_ptr<PreviewState>> previewStates;
    std::vector<std::unique_ptr<juce::AudioBuffer<float>>> previewBuffers;
    std::vector<std::unique_ptr<InstrumentPreviewSession>> instrumentSessions;
    std::vector<std::unique_ptr<juce::AudioBuffer<float>>> instrumentBuffers;
    std::array<PreviewVoiceRuntime, kPreviewVoiceCount> previewVoices;
    std::uint64_t previewSequence = 0;

    InstrumentPreviewSession* audioInstrumentSession = nullptr;
    InstrumentPreviewSession* audioInstrumentPendingSession = nullptr;
    juce::AudioBuffer<float>* audioInstrumentBuffer = nullptr;
    juce::AudioBuffer<float>* audioInstrumentPendingBuffer = nullptr;
    bool instrumentStopRequested = false;
    enum class InstrumentState { inactive, fadingIn, playing, fadingOut };
    InstrumentState instrumentState = InstrumentState::inactive;
    float instrumentGain = 0.0f;
    float instrumentFadeStep = 0.0f;
    float instrumentFadeAnchor[2] = {0.0f, 0.0f};
    float instrumentLastOutput[2] = {0.0f, 0.0f};

    std::array<SynthVoice, kSynthVoiceCount> synthVoices;
    std::atomic<bool> previewing{false};
    std::atomic<bool> instrumentPreviewing{false};
};

}  // namespace riffra
