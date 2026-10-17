#include <juce_audio_processors/juce_audio_processors.h>

#include <array>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>

#if defined(_WIN32)
#include <windows.h>
#else
#include <unistd.h>
#endif

namespace {

constexpr auto kCrtMarker = "riffra-test-plugin-stdout-crt";
constexpr auto kHandleMarker = "riffra-test-plugin-stdout-handle";

/// Writes through both standard output paths a hosted plugin can reach while
/// the sidecar isolation tests run: the CRT stream and the process standard
/// output handle.
void emitProtocolPollution() {
    const auto* enabled = std::getenv("RIFFRA_TEST_PLUGIN_STDOUT");
    if (enabled == nullptr || enabled[0] != '1') return;

    std::cout << kCrtMarker << std::endl;

    const std::string line = std::string(kHandleMarker) + '\n';
#if defined(_WIN32)
    DWORD written = 0;
    const auto handle = ::GetStdHandle(STD_OUTPUT_HANDLE);
    if (handle != nullptr && handle != INVALID_HANDLE_VALUE)
        ::WriteFile(handle, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
#else
    (void)::write(STDOUT_FILENO, line.data(), line.size());
#endif
}

}  // namespace

class ModeDependentParameter final : public juce::AudioParameterChoice {
public:
    explicit ModeDependentParameter(const juce::AudioParameterChoice& modeIn)
        : AudioParameterChoice("shape", "Shape", {"Small", "Medium", "Large"}, 0), mode(modeIn) {}

    juce::String getText(float normalizedValue, int) const override {
        const juce::StringArray labels = mode.getIndex() == 0
                                             ? juce::StringArray{"Small", "Medium", "Large"}
                                             : juce::StringArray{"Short", "Medium", "Long"};
        return labels[juce::jlimit(0, 2, juce::roundToInt(normalizedValue * 2.0f))];
    }

private:
    const juce::AudioParameterChoice& mode;
};

class RiffraTestProcessor final : public juce::AudioProcessor {
private:
    juce::AudioParameterFloat* gain = nullptr;
    std::array<std::array<float, 9>, 2> delay{};
    int delayPosition = 0;
    bool noteActive = false;

public:
    RiffraTestProcessor()
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)) {
        gain = new juce::AudioParameterFloat("gain", "Gain", 0.0f, 1.0f, 0.5f);
        addParameter(gain);
        addParameter(new juce::AudioParameterFloat(
            juce::ParameterID{"level", 1}, "Level", juce::NormalisableRange<float>{-60.0f, 0.0f},
            -6.0f, juce::AudioParameterFloatAttributes{}.withLabel("dB")));
        auto* mode = new juce::AudioParameterChoice("mode", "Mode", {"Clean", "Warm", "Bright"}, 0);
        addParameter(mode);
        addParameter(new ModeDependentParameter(*mode));
        if (!JucePlugin_IsSynth) setLatencySamples(8);
        emitProtocolPollution();
    }

    void prepareToPlay(double, int) override {
        delay = {};
        delayPosition = 0;
        noteActive = false;
    }
    void releaseResources() override {}

    bool isBusesLayoutSupported(const BusesLayout& layouts) const override {
        return layouts.getMainInputChannelSet() == juce::AudioChannelSet::stereo() &&
               layouts.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi) override {
        juce::ignoreUnused(midi);
        if (JucePlugin_IsSynth) {
            for (const auto event : midi) {
                const auto message = event.getMessage();
                if (message.isNoteOn()) noteActive = true;
                if (message.isNoteOff() || message.isAllNotesOff() || message.isAllSoundOff())
                    noteActive = false;
            }
            const auto value = noteActive ? 0.25f * gain->get() : 0.0f;
            for (int channel = 0; channel < buffer.getNumChannels(); ++channel)
                juce::FloatVectorOperations::fill(buffer.getWritePointer(channel), value,
                                                  buffer.getNumSamples());
        } else {
            for (int sample = 0; sample < buffer.getNumSamples(); ++sample) {
                const auto read = (delayPosition + 1) % 9;
                for (int channel = 0; channel < buffer.getNumChannels(); ++channel) {
                    delay[static_cast<std::size_t>(channel)]
                         [static_cast<std::size_t>(delayPosition)] =
                             buffer.getSample(channel, sample) * gain->get();
                    buffer.setSample(
                        channel, sample,
                        delay[static_cast<std::size_t>(channel)][static_cast<std::size_t>(read)]);
                }
                delayPosition = read;
            }
        }
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return JucePlugin_Name; }
    bool acceptsMidi() const override { return JucePlugin_WantsMidiInput; }
    bool producesMidi() const override { return JucePlugin_ProducesMidiOutput; }
    bool isMidiEffect() const override { return JucePlugin_IsMidiEffect; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock& state) override {
        std::array<float, 4> values{};
        for (int index = 0; index < 4; ++index)
            values[static_cast<std::size_t>(index)] = getParameters()[index]->getValue();
        state.replaceAll(values.data(), sizeof(values));
    }
    void setStateInformation(const void* data, const int size) override {
        std::array<float, 4> values{};
        if (size != sizeof(values)) return;
        std::memcpy(values.data(), data, sizeof(values));
        for (int index = 0; index < 4; ++index)
            getParameters()[index]->setValueNotifyingHost(values[static_cast<std::size_t>(index)]);
    }
};

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() { return new RiffraTestProcessor(); }
