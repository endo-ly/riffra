#include <juce_audio_processors/juce_audio_processors.h>

/// A sidechain-capable effect for routing tests: stereo main input and output
/// plus a stereo auxiliary input (input bus 1) that starts disabled. The
/// output adds the auxiliary signal so tests can observe sidechain delivery
/// and its sample alignment.
class RiffraTestSidechainProcessor final : public juce::AudioProcessor {
public:
    RiffraTestSidechainProcessor()
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withInput("Sidechain", juce::AudioChannelSet::stereo(), false)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)) {}

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}

    bool isBusesLayoutSupported(const BusesLayout& layouts) const override {
        if (layouts.getMainInputChannelSet() != juce::AudioChannelSet::stereo() ||
            layouts.getMainOutputChannelSet() != juce::AudioChannelSet::stereo())
            return false;
        const auto auxiliary = layouts.inputBuses[1];
        return auxiliary.isDisabled() || auxiliary == juce::AudioChannelSet::mono() ||
               auxiliary == juce::AudioChannelSet::stereo();
    }

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi) override {
        juce::ignoreUnused(midi);
        if (getBusCount(true) < 2 || !getBus(true, 1)->isEnabled()) return;
        const auto auxiliary = getBusBuffer(buffer, true, 1);
        auto output = getBusBuffer(buffer, false, 0);
        for (int channel = 0; channel < output.getNumChannels(); ++channel) {
            const auto source = juce::jmin(channel, auxiliary.getNumChannels() - 1);
            output.addFrom(channel, 0, auxiliary, source, 0, buffer.getNumSamples());
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
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}
};

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() {
    return new RiffraTestSidechainProcessor();
}
