#include <juce_audio_processors/juce_audio_processors.h>

#include <cstdlib>
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

class RiffraTestProcessor final : public juce::AudioProcessor {
public:
    RiffraTestProcessor()
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)) {
        emitProtocolPollution();
    }

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}

    bool isBusesLayoutSupported(const BusesLayout& layouts) const override {
        return layouts.getMainInputChannelSet() == juce::AudioChannelSet::stereo() &&
               layouts.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi) override {
        juce::ignoreUnused(midi);
        buffer.clear();
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
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}
};

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() { return new RiffraTestProcessor(); }
