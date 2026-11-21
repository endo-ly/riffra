#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <optional>
#include <thread>

#include "plugins/PluginEditorHost.h"
#include "support/TestAudioProcessor.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

}  // namespace

TEST(PluginEditorHostTest, DefersClosingCaptureUntilAutomationRestoresStaticValues) {
    juce::ScopedJuceInitialiser_GUI initialiser;
    ProcessorTrace trace;
    juce::String error;
    auto owned = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace, true, true),
                                             kSampleRate, kBlockSize, error);
    ASSERT_NE(owned, nullptr) << error;
    std::shared_ptr<PluginRack> rack(std::move(owned));
    const auto initial = rack->persistedState(error);
    ASSERT_TRUE(initial.has_value()) << error;
    ASSERT_TRUE(rack->applyPersistedState(*initial, error)) << error;
    auto parameters = rack->parameters();
    AutomationRuntime lane;
    lane.setPoints({{0, 0.9f}}, true);
    AutomationBindingFailure failure = AutomationBindingFailure::invalidParameter;
    auto binding =
        rack->bindAutomation(*parameters[0].parameterId, lane, *initial, false, failure, error);
    ASSERT_TRUE(binding.has_value()) << error;
    std::array<PluginParameterAutomation, 1> bindings{std::move(*binding)};
    std::array<float, kBlockSize> left{}, right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    std::optional<PluginStateSpec> captured;
    auto editor = std::make_shared<PluginEditorHost>(
        *rack, PluginEditorHost::StateCallback{},
        [&](const auto& state, const auto&) { captured = state; },
        PluginEditorHost::ParameterCallback{}, PluginEditorHost::ClosedCallback{}, rack);
    ASSERT_TRUE(editor->open(error)) << error;
    rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, nullptr, bindings, 0,
                  true);
    ASSERT_TRUE(rack->isAutomationActive());
    trace.opaqueState = 42;

    ASSERT_TRUE(editor->close());
    editor.reset();
    EXPECT_FALSE(captured.has_value());
    std::weak_ptr<PluginRack> retained = rack;
    rack.reset();
    auto pendingRack = retained.lock();
    ASSERT_NE(pendingRack, nullptr);
    pendingRack->restoreAutomation();
    pendingRack.reset();
    for (int attempt = 0; attempt < 100 && !captured.has_value(); ++attempt) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        juce::Timer::callPendingTimersSynchronously();
    }

    ASSERT_TRUE(captured.has_value());
    ASSERT_EQ(captured->parameterValues.size(), 1u);
    EXPECT_FLOAT_EQ(captured->parameterValues[0], initial->parameterValues[0]);
    ASSERT_TRUE(captured->stateData.has_value());
    EXPECT_EQ(*captured->stateData, juce::Base64::toBase64(&trace.opaqueState, sizeof(int)));
    EXPECT_TRUE(retained.expired());
}

}  // namespace riffra
