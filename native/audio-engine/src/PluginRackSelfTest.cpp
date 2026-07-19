#include <array>

#include "PluginRack.h"

namespace riffra {

namespace {

struct ProcessorTrace final {
    bool prepared = false;
    bool processed = false;
    bool released = false;
};

class TestProcessor final : public juce::AudioProcessor {
public:
    explicit TestProcessor(ProcessorTrace& processorTrace)
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
          trace(processorTrace) {}

    void prepareToPlay(double sampleRate, int samplesPerBlock) override {
        trace.prepared = sampleRate > 0.0 && samplesPerBlock > 0;
    }

    void releaseResources() override { trace.released = true; }

    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo() &&
               layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override {
        trace.processed = trace.prepared;
        buffer.applyGain(2.0f);
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "Riffra Test Processor"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    ProcessorTrace& trace;
};

juce::var check(const juce::String& name, const bool passed) {
    auto* result = new juce::DynamicObject();
    result->setProperty("name", name);
    result->setProperty("passed", passed);
    return juce::var(result);
}

}  // namespace

juce::Array<juce::var> runPluginRackSelfTests() {
    constexpr int blockSize = 32;
    ProcessorTrace trace;
    PluginRack rack;
    auto processor = std::make_unique<TestProcessor>(trace);
    const auto configurationError = PluginRack::configureProcessor(*processor, 48'000.0, blockSize);
    if (!configurationError) {
        rack.pluginInputChannels.store(processor->getMainBusNumInputChannels(),
                                       std::memory_order_release);
        rack.pluginOutputChannels.store(processor->getMainBusNumOutputChannels(),
                                        std::memory_order_release);
        rack.preparedSampleRate.store(48'000.0, std::memory_order_release);
        rack.preparedBlockSize.store(blockSize, std::memory_order_release);
        rack.plugin = std::move(processor);
        rack.loaded.store(true, std::memory_order_release);
    }

    std::array<float, blockSize> mono{};
    std::array<float, blockSize> left{};
    std::array<float, blockSize> right{};
    mono.fill(0.25f);
    const std::array<const float*, 1> inputs{mono.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    rack.process(inputs.data(), 1, outputs.data(), 2, blockSize);

    juce::Array<juce::var> checks;
    checks.add(check("Plugin layout is configured as stereo input and output",
                     !configurationError &&
                         rack.pluginInputChannels.load(std::memory_order_acquire) == 2 &&
                         rack.pluginOutputChannels.load(std::memory_order_acquire) == 2));
    checks.add(
        check("Plugin is prepared before DSP processing", trace.prepared && trace.processed));
    checks.add(check("Processed blocks report live DSP execution",
                     static_cast<juce::int64>(rack.status().getProperty("processedBlocks", 0)) ==
                         1));
    checks.add(check("Mono input reaches both processed output channels",
                     left.front() == 0.5f && right.front() == 0.5f && left.back() == 0.5f &&
                         right.back() == 0.5f));

    rack.setBypassed(true);
    rack.process(inputs.data(), 1, outputs.data(), 2, blockSize);
    checks.add(check("Plugin bypass returns dry mono input on both output channels",
                     left.front() == 0.25f && right.front() == 0.25f));
    const auto parameterStatus = rack.parameterStatus();
    checks.add(check("Parameter status does not capture plugin state",
                     parameterStatus.hasProperty("parameters") &&
                         !parameterStatus.hasProperty("stateData")));

    rack.clear();
    checks.add(check("Plugin resources are released before unload",
                     trace.released && !rack.status().getProperty("loaded", true)));
    return checks;
}

}  // namespace riffra
