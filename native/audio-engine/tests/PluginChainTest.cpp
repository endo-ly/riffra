#include "TestAudioProcessor.h"

#include "PluginChain.h"

#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <vector>

namespace riffra {

class PluginChainTestPeer final {
public:
    static bool addDevice(
        PluginChain& chain,
        const juce::String& id,
        std::unique_ptr<juce::AudioProcessor> processor,
        double sampleRate,
        int blockSize,
        juce::String& error) {
        auto rack = PluginRackTestPeer::install(
            std::move(processor), sampleRate, blockSize, error);
        if (rack == nullptr)
            return false;
        chain.devices.push_back(PluginChain::Device { id, std::move(rack) });
        return true;
    }
};

namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

std::unique_ptr<PluginChain> makeChain(
    std::vector<int>& order,
    juce::String& error,
    bool withTail = false) {
    auto chain = std::make_unique<PluginChain>();
    const std::array<float, 3> gains { 2.0f, 3.0f, 4.0f };
    const std::array<int, 3> latencies { 64, 128, 256 };
    const std::array<double, 3> tails = withTail
        ? std::array<double, 3> { 0.01, 0.02, 0.03 }
        : std::array<double, 3> { 0.0, 0.0, 0.0 };
    for (std::size_t index = 0; index < gains.size(); ++index) {
        if (!PluginChainTestPeer::addDevice(
                *chain,
                "device-" + juce::String(index + 1),
                std::make_unique<TestChainProcessor>(
                    static_cast<int>(index + 1), gains[index], latencies[index], order,
                    tails[index]),
                kSampleRate,
                kBlockSize,
                error))
            return {};
    }
    chain->prepare(kSampleRate, kBlockSize);
    return chain;
}

} // namespace

TEST(PluginChainTest, ProcessesDevicesInConfiguredOrder)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;

    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    input.fill(0.125f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    chain->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(order, (std::vector<int> { 1, 2, 3 }));
    EXPECT_FLOAT_EQ(outputLeft.front(), 3.0f);
    EXPECT_FLOAT_EQ(outputRight.back(), 3.0f);
}

TEST(PluginChainTest, BypassesOnlyRequestedDevice)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;
    ASSERT_TRUE(chain->setBypassed("device-2", true));

    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    input.fill(0.125f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    chain->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(order, (std::vector<int> { 1, 3 }));
    EXPECT_FLOAT_EQ(outputLeft.front(), 1.0f);
    EXPECT_FLOAT_EQ(outputRight.back(), 1.0f);
}

TEST(PluginChainTest, AggregatesDeviceLatency)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;

    EXPECT_EQ(chain->latencySamples(), 448);
}

TEST(PluginChainTest, AggregatesDeviceTail)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error, true);
    ASSERT_NE(chain, nullptr) << error;

    EXPECT_EQ(chain->tailSamples(), 2'880);
}

TEST(PluginChainTest, FindsDeviceById)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;

    EXPECT_NE(chain->findDevice("device-2"), nullptr);
    EXPECT_EQ(chain->findDevice("missing"), nullptr);
}

TEST(PluginChainTest, ClearsAllDevices)
{
    std::vector<int> order;
    juce::String error;
    auto chain = makeChain(order, error);
    ASSERT_NE(chain, nullptr) << error;
    ASSERT_EQ(chain->size(), 3);

    chain->clear();

    EXPECT_EQ(chain->size(), 0);
    EXPECT_EQ(chain->findDevice("device-1"), nullptr);
}

TEST(PluginChainTest, MirrorsPersistedStateAndQueuedParameters)
{
    PluginChain playbackState;
    PluginChain liveState;
    PluginChain recordingState;
    juce::String error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        playbackState, "device-state", std::make_unique<StateTestProcessor>(),
        kSampleRate, kBlockSize, error)) << error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        liveState, "device-state", std::make_unique<StateTestProcessor>(),
        kSampleRate, kBlockSize, error)) << error;
    ASSERT_TRUE(PluginChainTestPeer::addDevice(
        recordingState, "device-state", std::make_unique<StateTestProcessor>(),
        kSampleRate, kBlockSize, error)) << error;

    ASSERT_TRUE(playbackState.setParameter("device-state", 0, 0.75f, error));
    const auto captured = playbackState.persistedState("device-state", error);
    ASSERT_TRUE(captured.isObject()) << error;
    auto* persistedDevice = new juce::DynamicObject();
    persistedDevice->setProperty("id", "device-state");
    persistedDevice->setProperty("kind", "plugin");
    persistedDevice->setProperty(
        "parameterValues", captured.getProperty("parameterValues", juce::Array<juce::var> {}));
    persistedDevice->setProperty("stateData", captured.getProperty("stateData", {}));
    persistedDevice->setProperty("bypassed", captured.getProperty("bypassed", false));
    juce::Array<juce::var> persistedDevices;
    persistedDevices.add(juce::var(persistedDevice));

    ASSERT_TRUE(liveState.applyState(juce::var(persistedDevices), error)) << error;
    const auto liveCaptured = liveState.persistedState("device-state", error);
    const auto liveValues = liveCaptured.getProperty(
        "parameterValues", juce::Array<juce::var> {});
    ASSERT_TRUE(liveValues.isArray());
    ASSERT_EQ(liveValues.size(), 700);
    EXPECT_NEAR(static_cast<float>(liveValues[0]), 0.75f, 0.0001f);

    liveState.prepare(kSampleRate, kBlockSize);
    recordingState.prepare(kSampleRate, kBlockSize);
    const std::array<std::pair<int, float>, 4> queuedParameters {
        std::pair { 0, 0.10f },
        std::pair { 511, 0.20f },
        std::pair { 512, 0.30f },
        std::pair { 699, 0.40f },
    };
    for (const auto [index, value] : queuedParameters) {
        ASSERT_TRUE(liveState.findDevice("device-state") != nullptr);
        ASSERT_TRUE(recordingState.findDevice("device-state") != nullptr);
        liveState.findDevice("device-state")->enqueueParameterChange(index, value);
        recordingState.findDevice("device-state")->enqueueParameterChange(index, value);
    }

    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    liveState.process(nullptr, 0, outputs.data(), 2, kBlockSize);
    recordingState.process(nullptr, 0, outputs.data(), 2, kBlockSize);
    const auto liveQueued = liveState.persistedState("device-state", error)
                                .getProperty("parameterValues", juce::Array<juce::var> {});
    const auto recordingQueued = recordingState.persistedState("device-state", error)
                                      .getProperty("parameterValues", juce::Array<juce::var> {});
    ASSERT_TRUE(liveQueued.isArray());
    ASSERT_TRUE(recordingQueued.isArray());
    ASSERT_EQ(liveQueued.size(), 700);
    ASSERT_EQ(recordingQueued.size(), 700);
    for (const auto [index, value] : queuedParameters) {
        EXPECT_NEAR(static_cast<float>(liveQueued[index]), value, 0.0001f);
        EXPECT_NEAR(static_cast<float>(recordingQueued[index]), value, 0.0001f);
    }
}

} // namespace riffra
