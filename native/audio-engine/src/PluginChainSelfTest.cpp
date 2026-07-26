#include <array>
#include <cstring>
#include <utility>
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

class StateTestProcessor final : public juce::AudioProcessor {
public:
    StateTestProcessor()
        : AudioProcessor(BusesProperties()
              .withInput("Input", juce::AudioChannelSet::stereo(), true)
              .withOutput("Output", juce::AudioChannelSet::stereo(), true)) {
        parameters.reserve(700);
        for (int index = 0; index < 700; ++index) {
            auto* parameter = new juce::AudioParameterFloat(
                "state" + juce::String(index),
                "State " + juce::String(index),
                0.0f,
                1.0f,
                0.0f);
            parameters.push_back(parameter);
            addParameter(parameter);
        }
    }

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}
    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo()
            && layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }
    void processBlock(juce::AudioBuffer<float>&, juce::MidiBuffer&) override {}
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "State Test Processor"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock& state) override {
        for (const auto* parameter : getParameters()) {
            const auto normalized = parameter->getValue();
            state.append(&normalized, sizeof(normalized));
        }
    }
    void setStateInformation(const void* data, const int size) override {
        if (data == nullptr || size != static_cast<int>(parameters.size() * sizeof(float)))
            return;
        const auto* values = static_cast<const float*>(data);
        for (std::size_t index = 0; index < parameters.size(); ++index)
            parameters[index]->setValueNotifyingHost(values[index]);
    }

private:
    std::vector<juce::AudioParameterFloat*> parameters;
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

    PluginChain playbackState;
    PluginChain liveState;
    PluginChain recordingState;
    const auto addStateRack = [](PluginChain& target) {
        auto rack = std::make_unique<PluginRack>();
        auto processor = std::make_unique<StateTestProcessor>();
        rack->pluginInputChannels.store(2, std::memory_order_release);
        rack->pluginOutputChannels.store(2, std::memory_order_release);
        rack->plugin = std::move(processor);
        rack->loaded.store(true, std::memory_order_release);
        target.devices.push_back(
            PluginChain::Device { "device-state", std::move(rack) });
    };
    addStateRack(playbackState);
    addStateRack(liveState);
    addStateRack(recordingState);
    juce::String stateError;
    const auto changed = playbackState.setParameter(
        "device-state", 0, 0.75f, stateError);
    const auto captured = playbackState.persistedState(
        "device-state", stateError);
    auto* persistedDevice = new juce::DynamicObject();
    persistedDevice->setProperty("id", "device-state");
    persistedDevice->setProperty("kind", "plugin");
    persistedDevice->setProperty(
        "parameterValues",
        captured.getProperty("parameterValues", juce::Array<juce::var> {}));
    persistedDevice->setProperty(
        "stateData", captured.getProperty("stateData", {}));
    persistedDevice->setProperty(
        "bypassed", captured.getProperty("bypassed", false));
    juce::Array<juce::var> persistedDevices;
    persistedDevices.add(juce::var(persistedDevice));
    const auto* liveRackForState = liveState.findDevice("device-state");
    const auto liveStatusBeforeStateApply = liveRackForState != nullptr
        ? liveRackForState->status()
        : juce::var {};
    const auto mirrored = liveState.applyState(
        juce::var(persistedDevices), stateError);
    const auto liveCaptured = liveState.persistedState(
        "device-state", stateError);
    const auto liveStatusAfterStateApply = liveRackForState != nullptr
        ? liveRackForState->status()
        : juce::var {};
    const auto liveValues = liveCaptured.getProperty(
        "parameterValues", juce::Array<juce::var> {});
    const auto stateCaptured = changed && captured.isObject()
        && captured.getProperty("stateData", {}).toString().isNotEmpty();
    checks.add(check(
        "Plugin Editor parameter and opaque state are captured",
        stateCaptured));
    checks.add(check(
        "Persisted Plugin state applies to the Live Chain",
        mirrored && stateError.isEmpty()));
    checks.add(check(
        "Opaque Plugin state changes do not add transition blocks",
        static_cast<juce::int64>(liveStatusAfterStateApply.getProperty(
            "transitionBlocks", -1))
            == static_cast<juce::int64>(liveStatusBeforeStateApply.getProperty(
                "transitionBlocks", -1))));
    const auto liveValue = liveValues.isArray() && liveValues.size() == 700
        ? static_cast<float>(liveValues[0])
        : -1.0f;
    auto mirroredCheck = check(
        "Plugin Editor state is mirrored into the Live Chain",
        liveValues.isArray()
            && liveValues.size() == 700
            && std::abs(liveValue - 0.75f) < 0.0001f);
    mirroredCheck.getDynamicObject()->setProperty("value", liveValue);
    checks.add(mirroredCheck);
    liveState.prepare(sampleRate, blockSize);
    recordingState.prepare(sampleRate, blockSize);
    const std::array<std::pair<int, float>, 4> queuedParameters {
        std::pair { 0, 0.10f },
        std::pair { 511, 0.20f },
        std::pair { 512, 0.30f },
        std::pair { 699, 0.40f },
    };
    if (auto* liveRack = liveState.findDevice("device-state"))
        for (const auto [index, value] : queuedParameters)
            liveRack->enqueueParameterChange(index, value);
    if (auto* recordingRack = recordingState.findDevice("device-state"))
        for (const auto [index, value] : queuedParameters)
            recordingRack->enqueueParameterChange(index, value);
    liveState.process(inputs.data(), 1, outputs.data(), 2, blockSize);
    recordingState.process(inputs.data(), 1, outputs.data(), 2, blockSize);
    const auto queuedCaptured = liveState.persistedState("device-state", stateError);
    const auto queuedValues = queuedCaptured.getProperty(
        "parameterValues", juce::Array<juce::var> {});
    const auto recordingCaptured = recordingState.persistedState("device-state", stateError);
    const auto recordingValues = recordingCaptured.getProperty(
        "parameterValues", juce::Array<juce::var> {});
    checks.add(check(
        "700 Plugin Editor parameters reach Live and Recording Chains at a block boundary",
        queuedValues.isArray()
            && queuedValues.size() == 700
            && std::abs(static_cast<float>(queuedValues[0]) - 0.10f) < 0.0001f
            && std::abs(static_cast<float>(queuedValues[511]) - 0.20f) < 0.0001f
            && std::abs(static_cast<float>(queuedValues[512]) - 0.30f) < 0.0001f
            && std::abs(static_cast<float>(queuedValues[699]) - 0.40f) < 0.0001f
            && recordingValues.isArray()
            && recordingValues.size() == 700
            && std::abs(static_cast<float>(recordingValues[0]) - 0.10f) < 0.0001f
            && std::abs(static_cast<float>(recordingValues[511]) - 0.20f) < 0.0001f
            && std::abs(static_cast<float>(recordingValues[512]) - 0.30f) < 0.0001f
            && std::abs(static_cast<float>(recordingValues[699]) - 0.40f) < 0.0001f));
    return checks;
}

}  // namespace riffra
