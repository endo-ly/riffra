#include <array>
#include <vector>

#include "PluginChain.h"

namespace riffra {

namespace {

class ChainTestProcessor final : public juce::AudioProcessor {
public:
    ChainTestProcessor(const int processorId, const float processorGain, const int latency,
                       std::vector<int>& processorOrder)
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
          id(processorId),
          gain(processorGain),
          order(processorOrder) {
        setLatencySamples(latency);
    }

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}

    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo() &&
               layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override {
        order.push_back(id);
        buffer.applyGain(gain);
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "Riffra Chain Test Processor"; }
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
    int id;
    float gain;
    std::vector<int>& order;
};

juce::var check(const juce::String& name, const bool passed) {
    auto* result = new juce::DynamicObject();
    result->setProperty("name", name);
    result->setProperty("passed", passed);
    return juce::var(result);
}

}  // namespace

juce::Array<juce::var> runPluginChainSelfTests() {
    constexpr int blockSize = 32;
    constexpr double sampleRate = 48'000.0;
    const std::array<float, 3> gains{2.0f, 3.0f, 4.0f};
    const std::array<int, 3> latencies{64, 128, 256};
    std::vector<int> processOrder;
    PluginChain chain;
    juce::String configurationError;

    for (std::size_t index = 0; index < gains.size(); ++index) {
        auto rack = std::make_unique<PluginRack>();
        auto processor = std::make_unique<ChainTestProcessor>(
            static_cast<int>(index + 1), gains[index], latencies[index], processOrder);
        if (const auto error = PluginRack::configureProcessor(*processor, sampleRate, blockSize)) {
            configurationError = error->message;
            break;
        }
        rack->pluginInputChannels.store(processor->getMainBusNumInputChannels(),
                                        std::memory_order_release);
        rack->pluginOutputChannels.store(processor->getMainBusNumOutputChannels(),
                                         std::memory_order_release);
        rack->preparedSampleRate.store(sampleRate, std::memory_order_release);
        rack->preparedBlockSize.store(blockSize, std::memory_order_release);
        rack->plugin = std::move(processor);
        rack->loaded.store(true, std::memory_order_release);
        chain.devices.push_back(
            PluginChain::Device{"device-" + juce::String(index + 1), std::move(rack)});
    }
    chain.prepare(sampleRate, blockSize);

    std::array<float, blockSize> input{};
    std::array<float, blockSize> left{};
    std::array<float, blockSize> right{};
    input.fill(0.125f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    chain.process(inputs.data(), 1, outputs.data(), 2, blockSize);

    juce::Array<juce::var> checks;
    checks.add(check("Three Plugins process serially in configured order",
                     configurationError.isEmpty() && chain.size() == 3 &&
                         processOrder == std::vector<int>({1, 2, 3}) && left.front() == 3.0f &&
                         right.back() == 3.0f));
    checks.add(check("Plugin Chain latency is the sum of all device latencies",
                     chain.latencySamples() == 448));

    processOrder.clear();
    const auto bypassed = chain.setBypassed("device-2", true);
    chain.process(inputs.data(), 1, outputs.data(), 2, blockSize);
    checks.add(check("Device bypass preserves Chain identity and order",
                     bypassed && processOrder == std::vector<int>({1, 3}) && left.front() == 1.0f &&
                         right.back() == 1.0f));
    return checks;
}

}  // namespace riffra
