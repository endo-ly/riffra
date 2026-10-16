#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <vector>

#include "../support/TestAudioProcessor.h"
#include "plugins/PluginChain.h"

namespace riffra {

class PluginChainTestPeer final {
public:
    static bool addDevice(PluginChain& chain, const juce::String& id,
                          std::unique_ptr<juce::AudioProcessor> processor, double sampleRate,
                          int blockSize, juce::String& error, bool sidechain = false) {
        auto rack = PluginRackTestPeer::install(std::move(processor), sampleRate, blockSize, error,
                                                false, sidechain);
        if (rack == nullptr) return false;
        chain.prepareBuffers(blockSize);
        chain.devices.push_back(PluginChain::Device{id, std::move(rack), {}});
        if (sidechain) chain.prepareSidechainInput(chain.devices.back(), blockSize);
        return true;
    }

    static void setSidechainTargets(PluginChain& chain,
                                    const std::vector<SidechainSendSpec>& sidechainSends) {
        chain.sidechainTargets = PluginChain::enabledSidechainTargets(sidechainSends);
    }

    static juce::AudioBuffer<float>* sidechainBuffer(PluginChain& chain, const juce::String& id) {
        for (auto& device : chain.devices)
            if (device.id == id) return &device.sidechainBuffer;
        return nullptr;
    }
};

namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

std::unique_ptr<PluginChain> makeChain(std::vector<int>& order, juce::String& error,
                                       bool withTail = false) {
    auto chain = std::make_unique<PluginChain>();
    const std::array<float, 3> gains{2.0f, 3.0f, 4.0f};
    const std::array<int, 3> latencies{64, 128, 256};
    const std::array<double, 3> tails =
        withTail ? std::array<double, 3>{0.01, 0.02, 0.03} : std::array<double, 3>{0.0, 0.0, 0.0};
    for (std::size_t index = 0; index < gains.size(); ++index) {
        if (!PluginChainTestPeer::addDevice(
                *chain, "device-" + juce::String(index + 1),
                std::make_unique<TestChainProcessor>(static_cast<int>(index + 1), gains[index],
                                                     latencies[index], order, tails[index]),
                kSampleRate, kBlockSize, error))
            return {};
    }
    return chain;
}

}  // namespace

TEST(PluginChainTest, ProcessesDevicesInConfiguredOrder) {
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;

    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    input.fill(0.125f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    chain->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(order, (std::vector<int>{1, 2, 3}));
    EXPECT_FLOAT_EQ(outputLeft.front(), 3.0f);
    EXPECT_FLOAT_EQ(outputRight.back(), 3.0f);
}

TEST(PluginChainTest, BypassesOnlyRequestedDevice) {
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;
    ASSERT_TRUE(chain->setBypassed("device-2", true));

    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    input.fill(0.125f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    chain->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(order, (std::vector<int>{1, 3}));
    EXPECT_FLOAT_EQ(outputLeft.front(), 1.0f);
    EXPECT_FLOAT_EQ(outputRight.back(), 1.0f);
}

TEST(PluginChainTest, AggregatesDeviceLatencyAndTail) {
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error, true);
    ASSERT_NE(chain, nullptr) << error;

    EXPECT_EQ(chain->latencySamples(), 448);
    EXPECT_EQ(chain->tailSamples(), 2'880);
}

TEST(PluginChainTest, DelaysDeviceMainInputsByThePreparedAlignment) {
    std::vector<int> order;
    juce::String error;
    PluginChain chain;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        chain, "device-1", std::make_unique<TestChainProcessor>(1, 1.0f, 0, order), kSampleRate,
        kBlockSize, error))
        << error;

    const std::vector<std::pair<juce::String, int>> alignment{{"device-1", 4}};
    ASSERT_TRUE(chain.prepareMainInputDelays(alignment, error)) << error;
    EXPECT_TRUE(chain.usesSameMainInputDelays(alignment));
    EXPECT_TRUE(chain.usesSameMainInputDelays({{"device-1", 4}, {"device-2", 0}}));
    EXPECT_FALSE(chain.usesSameMainInputDelays({{"device-1", 3}}));
    EXPECT_EQ(chain.latencySamples(), 4);

    std::array<float, 8> input{};
    std::array<float, 8> outputLeft{};
    std::array<float, 8> outputRight{};
    input[0] = 1.0f;
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    chain.process(inputs.data(), 1, outputs.data(), 2, 8);

    for (int sample = 0; sample < 4; ++sample) {
        EXPECT_FLOAT_EQ(outputLeft[static_cast<std::size_t>(sample)], 0.0f) << sample;
        EXPECT_FLOAT_EQ(outputRight[static_cast<std::size_t>(sample)], 0.0f) << sample;
    }
    EXPECT_FLOAT_EQ(outputLeft[4], 1.0f);
    EXPECT_FLOAT_EQ(outputRight[4], 1.0f);

    EXPECT_FALSE(chain.prepareMainInputDelays({{"device-1", 8}}, error));
    EXPECT_EQ(error, "Plugin Chain main input delays differ from the prepared alignment.");

    PluginChain unknown;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        unknown, "device-1", std::make_unique<TestChainProcessor>(1, 1.0f, 0, order), kSampleRate,
        kBlockSize, error))
        << error;
    EXPECT_FALSE(unknown.prepareMainInputDelays({{"device-2", 1}}, error));
    EXPECT_EQ(error, "Plugin Chain main input delay references an unknown device.");
}

TEST(PluginChainTest, MirrorsPersistedStateAndQueuedParameters) {
    PluginChain playbackState;
    PluginChain liveState;
    PluginChain recordingState;
    juce::String error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(playbackState, "device-state",
                                               std::make_unique<StateTestProcessor>(), kSampleRate,
                                               kBlockSize, error))
        << error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(liveState, "device-state",
                                               std::make_unique<StateTestProcessor>(), kSampleRate,
                                               kBlockSize, error))
        << error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(recordingState, "device-state",
                                               std::make_unique<StateTestProcessor>(), kSampleRate,
                                               kBlockSize, error))
        << error;

    juce::AudioBuffer<float> output(2, kBlockSize);
    const auto process = [&](PluginChain& chain) {
        chain.process(nullptr, 0, output.getArrayOfWritePointers(), 2, kBlockSize);
    };
    ASSERT_TRUE(playbackState.setParameter("device-state", 0, 0.75f, error));
    process(playbackState);
    const auto captured = playbackState.persistedState("device-state", error);
    ASSERT_TRUE(captured.has_value()) << error;
    const std::vector<PluginDeviceSpec> persistedDevices{
        {"device-state", "test-device.vst3", *captured}};

    ASSERT_TRUE(liveState.applyState(persistedDevices, error)) << error;
    process(liveState);
    const auto liveCaptured = liveState.persistedState("device-state", error);
    ASSERT_TRUE(liveCaptured.has_value()) << error;
    const auto& liveValues = liveCaptured->parameterValues;
    ASSERT_EQ(liveValues.size(), 700u);
    EXPECT_NEAR(liveValues[0], 0.75f, 0.0001f);

    const std::array<std::pair<int, float>, 4> queuedParameters{
        std::pair{0, 0.10f},
        std::pair{511, 0.20f},
        std::pair{512, 0.30f},
        std::pair{699, 0.40f},
    };
    for (const auto [index, value] : queuedParameters) {
        ASSERT_TRUE(liveState.findDevice("device-state") != nullptr);
        ASSERT_TRUE(recordingState.findDevice("device-state") != nullptr);
        liveState.findDevice("device-state")->enqueueParameterChange(index, value);
        recordingState.findDevice("device-state")->enqueueParameterChange(index, value);
    }

    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    liveState.process(nullptr, 0, outputs.data(), 2, kBlockSize);
    recordingState.process(nullptr, 0, outputs.data(), 2, kBlockSize);
    const auto liveQueued = liveState.persistedState("device-state", error);
    const auto recordingQueued = recordingState.persistedState("device-state", error);
    ASSERT_TRUE(liveQueued.has_value());
    ASSERT_TRUE(recordingQueued.has_value());
    ASSERT_EQ(liveQueued->parameterValues.size(), 700u);
    ASSERT_EQ(recordingQueued->parameterValues.size(), 700u);
    for (const auto [index, value] : queuedParameters) {
        const auto offset = static_cast<std::size_t>(index);
        EXPECT_NEAR(liveQueued->parameterValues[offset], value, 0.0001f);
        EXPECT_NEAR(recordingQueued->parameterValues[offset], value, 0.0001f);
    }
}

TEST(PluginChainTest, PassesSidechainAudioOnlyToTheTargetedDevice) {
    SidechainTrace plainTrace;
    SidechainTrace targetTrace;
    juce::String error;
    PluginChain chain;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        chain, "device-plain", std::make_unique<TestSidechainProcessor>(true, false, plainTrace),
        kSampleRate, kBlockSize, error))
        << error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        chain, "device-target", std::make_unique<TestSidechainProcessor>(true, false, targetTrace),
        kSampleRate, kBlockSize, error, true))
        << error;

    auto* aux = PluginChainTestPeer::sidechainBuffer(chain, "device-target");
    ASSERT_NE(aux, nullptr);
    ASSERT_EQ(aux->getNumChannels(), 2);
    juce::FloatVectorOperations::fill(aux->getWritePointer(0), 0.25f, kBlockSize);
    juce::FloatVectorOperations::fill(aux->getWritePointer(1), 0.25f, kBlockSize);

    std::array<float, kBlockSize> inputLeft{};
    std::array<float, kBlockSize> inputRight{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    inputLeft.fill(0.125f);
    inputRight.fill(0.125f);
    const std::array<const float*, 2> inputs{inputLeft.data(), inputRight.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    chain.process(inputs.data(), 2, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(plainTrace.auxBusChannels, 0);
    EXPECT_FLOAT_EQ(plainTrace.auxPeak, 0.0f);
    EXPECT_EQ(targetTrace.auxBusChannels, 1);
    EXPECT_FLOAT_EQ(targetTrace.auxPeak, 0.25f);
    EXPECT_FLOAT_EQ(outputLeft.front(), 0.375f);
    EXPECT_FLOAT_EQ(outputRight.front(), 0.375f);
}

TEST(PluginChainTest, RepreparesWhenEnabledSidechainTargetsChange) {
    const SidechainSendSpec send{"send-1", "track-1", "device-a", -6.0, SendPositionSpec::postFader,
                                 true};
    PluginChain chain;
    PluginChainTestPeer::setSidechainTargets(chain, {send});
    EXPECT_TRUE(chain.usesSameSidechainTargets({send}));

    auto leveled = send;
    leveled.levelDb = -12.0;
    EXPECT_TRUE(chain.usesSameSidechainTargets({leveled}));

    auto disabled = send;
    disabled.enabled = false;
    EXPECT_FALSE(chain.usesSameSidechainTargets({disabled}));
    EXPECT_FALSE(chain.usesSameSidechainTargets({}));

    const SidechainSendSpec otherTarget{
        "send-2", "track-1", "device-b", 0.0, SendPositionSpec::preFader, true};
    EXPECT_FALSE(chain.usesSameSidechainTargets({send, otherTarget}));

    const SidechainSendSpec secondSource{
        "send-3", "track-2", "device-a", 0.0, SendPositionSpec::postFader, true};
    EXPECT_TRUE(chain.usesSameSidechainTargets({send, secondSource}));
}

}  // namespace riffra
