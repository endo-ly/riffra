#include <gtest/gtest.h>

#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <thread>
#include <utility>
#include <vector>

#include "rig/RigEngine.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 256;
/// Gain every instance of the test effect applies to its input.
constexpr float kTestEffectGain = 0.5f;

RigProjectionSpec projection(std::vector<PluginDeviceSpec> effects, const double gainDb = 0.0,
                             const std::uint32_t channel = 0) {
    return RigProjectionSpec{"rig-1", 1, channel, gainDb, std::move(effects)};
}

PluginDeviceSpec effect(const juce::String& id,
                        const juce::String& path = RIFFRA_TEST_EFFECT_BUNDLE_PATH,
                        const bool bypassed = false) {
    PluginStateSpec state;
    state.bypassed = bypassed;
    return PluginDeviceSpec{id, path, std::move(state)};
}

std::array<std::array<float, kBlockSize>, 2> mixOnce(RigEngine& engine, const float first,
                                                     const float second,
                                                     const double sampleRate = kSampleRate) {
    std::array<std::array<float, kBlockSize>, 2> input{};
    input[0].fill(first);
    input[1].fill(second);
    std::array<std::array<float, kBlockSize>, 2> output{};
    output[0].fill(0.0f);
    output[1].fill(0.0f);
    const std::array<const float*, 2> inputs{input[0].data(), input[1].data()};
    const std::array<float*, 2> outputs{output[0].data(), output[1].data()};
    engine.mix(inputs.data(), 2, outputs.data(), 2, kBlockSize, sampleRate);
    return output;
}

}  // namespace

TEST(RigEngineTest, PlaysItsOwnInputChannelThroughEveryEffectAtTheRigGain) {
    // Arrange
    RigEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.install(projection({effect("effect:1"), effect("effect:2")}, -6.0, 1),
                               kSampleRate, kBlockSize, error))
        << error;

    // Act
    const auto output = mixOnce(engine, 0.5f, 0.1f);
    const auto otherFormat = mixOnce(engine, 0.5f, 0.1f, 44'100.0);

    // Assert: the whole chain runs on the rig's own input channel.
    const auto expected =
        0.1f * kTestEffectGain * kTestEffectGain * juce::Decibels::decibelsToGain(-6.0f);
    EXPECT_TRUE(engine.monitorsInput());
    EXPECT_EQ(engine.inputChannel(), 1u);
    EXPECT_NEAR(output[0][kBlockSize - 1], expected, 1e-6f);
    EXPECT_NEAR(output[1][kBlockSize - 1], expected, 1e-6f);
    EXPECT_FLOAT_EQ(otherFormat[0][kBlockSize - 1], 0.0f);
}

TEST(RigEngineTest, ReusesTheChainWhenTheEffectOrderIsUnchanged) {
    // Arrange: the second instance holds the state its editor captured.
    RigEngine engine;
    juce::String error;
    PluginDeviceSpec live = effect("effect:2");
    live.state.parameterValues = {1.0f};
    ASSERT_TRUE(engine.install(projection({effect("effect:1"), std::move(live)}), kSampleRate,
                               kBlockSize, error))
        << error;
    const auto first = engine.findRack("effect:1");
    const auto second = engine.findRack("effect:2");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(second, nullptr);

    // Act: the same order carries a new bypass and gain, and a state the store
    // still held before the editor captured 1.0.
    PluginDeviceSpec stored = effect("effect:2");
    stored.state.parameterValues = {0.0f};
    ASSERT_TRUE(engine.install(
        projection({effect("effect:1", RIFFRA_TEST_EFFECT_BUNDLE_PATH, true), std::move(stored)},
                   -3.0),
        kSampleRate, kBlockSize, error))
        << error;
    const auto bypassed = engine.findRack("effect:1");
    const auto retained = engine.findRack("effect:2");
    ASSERT_NE(bypassed, nullptr);
    ASSERT_NE(retained, nullptr);
    const auto state = retained->persistedState(error);
    const auto output = mixOnce(engine, 0.25f, 0.0f);

    // Assert: the prepared instances keep playing, and the stored state did not
    // replace the state the instance already holds.
    EXPECT_EQ(bypassed.get(), first.get());
    EXPECT_EQ(retained.get(), second.get());
    EXPECT_TRUE(bypassed->status().bypassed);
    ASSERT_TRUE(state.has_value()) << error;
    EXPECT_FLOAT_EQ(state->parameterValues.front(), 1.0f);
    const auto expected = 0.25f * juce::Decibels::decibelsToGain(-3.0f);
    EXPECT_NEAR(output[0][kBlockSize - 1], expected, 1e-6f);
}

TEST(RigEngineTest, RebuildsTheChainWhenTheEffectOrderChanges) {
    // Arrange
    RigEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.install(projection({effect("effect:1"), effect("effect:2")}), kSampleRate,
                               kBlockSize, error))
        << error;
    const auto first = engine.findRack("effect:1");
    const auto second = engine.findRack("effect:2");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(second, nullptr);

    // Act
    ASSERT_TRUE(engine.install(projection({effect("effect:2"), effect("effect:1")}), kSampleRate,
                               kBlockSize, error))
        << error;
    const auto reordered = engine.findRack("effect:1");
    const auto moved = engine.findRack("effect:2");

    // Assert
    EXPECT_NE(reordered.get(), first.get());
    EXPECT_NE(moved.get(), second.get());
}

TEST(RigEngineTest, RebuildsTheChainWhenAnotherRigCarriesTheSameEffects) {
    // Arrange
    RigEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.install(projection({effect("effect:1")}), kSampleRate, kBlockSize, error))
        << error;
    const auto first = engine.findRack("effect:1");
    ASSERT_NE(first, nullptr);

    // Act: another rig has its own state, so it must not inherit the prepared
    // instances of the rig that played.
    auto other = projection({effect("effect:1")});
    other.rigId = "rig-2";
    ASSERT_TRUE(engine.install(other, kSampleRate, kBlockSize, error)) << error;
    const auto replaced = engine.findRack("effect:1");

    // Assert
    ASSERT_NE(replaced, nullptr);
    EXPECT_NE(replaced.get(), first.get());
    EXPECT_EQ(engine.rigId(), juce::String("rig-2"));
}

TEST(RigEngineTest, RejectsDuplicateDeviceIds) {
    // Arrange
    RigEngine engine;
    juce::String error;
    const RigProjectionSpec invalid = projection({effect("effect:1"), effect("effect:1")});

    // Act
    const auto installed = engine.install(invalid, kSampleRate, kBlockSize, error);

    // Assert
    EXPECT_FALSE(installed);
    EXPECT_FALSE(error.isEmpty());
    EXPECT_EQ(engine.findRack("effect:1"), nullptr);
}

TEST(RigEngineTest, FadesTheLastBlockToSilenceWhenUninstalledDuringPlayback) {
    // Arrange
    RigEngine engine;
    juce::String error;
    ASSERT_TRUE(engine.install(projection({effect("effect:1")}), kSampleRate, kBlockSize, error))
        << error;
    std::atomic<bool> stop{false};
    std::atomic<int> mixedBlocks{0};
    std::array<std::array<float, kBlockSize>, 2> lastAudible{};
    // Prime the effect so the measured block starts from a filled delay line.
    mixOnce(engine, 0.25f, 0.25f);
    std::thread audio([&] {
        while (!stop.load(std::memory_order_acquire)) {
            const auto output = mixOnce(engine, 0.25f, 0.25f);
            if (std::abs(output[0][0]) > 0.0f) lastAudible = output;
            mixedBlocks.fetch_add(1, std::memory_order_release);
        }
    });
    // Uninstalling waits for a fade the audio thread can only produce once it
    // runs, so the first blocks are awaited instead of assumed.
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (mixedBlocks.load(std::memory_order_acquire) < 2 &&
           std::chrono::steady_clock::now() < deadline)
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    ASSERT_GE(mixedBlocks.load(std::memory_order_acquire), 2);

    // Act
    engine.uninstall();
    stop.store(true, std::memory_order_release);
    audio.join();

    // Assert
    EXPECT_NEAR(lastAudible[0][0], 0.25f * kTestEffectGain * (1.0f - 1.0f / kBlockSize), 1e-4f);
    EXPECT_FLOAT_EQ(lastAudible[0][kBlockSize - 1], 0.0f);
    EXPECT_FALSE(engine.monitorsInput());
    EXPECT_EQ(engine.findRack("effect:1"), nullptr);
}

}  // namespace riffra
