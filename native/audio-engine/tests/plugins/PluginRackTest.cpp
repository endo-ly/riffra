#include <gtest/gtest.h>

#include <array>
#include <cmath>

#include "../support/TestAudioProcessor.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

std::unique_ptr<PluginRack> makeRack(ProcessorTrace& trace, juce::String& error) {
    return PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), kSampleRate,
                                       kBlockSize, error);
}

}  // namespace

TEST(PluginRackTest, ProcessesMonoInputToStereoOutput) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.5f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.5f);
}

TEST(PluginRackTest, ReturnsDrySignalWhenBypassedOrUnloaded) {
    const auto expectDrySignal = [](PluginRack& rack) {
        std::array<float, kBlockSize> input{};
        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        input.fill(0.25f);
        const std::array<const float*, 1> inputs{input.data()};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
        rack.process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

        EXPECT_FLOAT_EQ(outputLeft.front(), 0.25f);
        EXPECT_FLOAT_EQ(outputRight.back(), 0.25f);
    };

    ProcessorTrace trace;
    juce::String error;
    auto bypassed = makeRack(trace, error);
    ASSERT_NE(bypassed, nullptr) << error;
    bypassed->setBypassed(true);
    expectDrySignal(*bypassed);

    PluginRack unloaded;
    expectDrySignal(unloaded);
}

TEST(PluginRackTest, ChangesAndReportsPluginPrograms) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    const auto initial = rack->programStatus();
    ASSERT_EQ(initial.names.size(), 2u);
    EXPECT_EQ(initial.currentIndex, 0);
    EXPECT_EQ(initial.names[0], "Program 0");

    EXPECT_TRUE(rack->setProgram(1, error)) << error;
    EXPECT_EQ(trace.currentProgram, 1);
    const auto changed = rack->programStatus();
    EXPECT_EQ(changed.currentIndex, 1);
    EXPECT_EQ(changed.names[1], "Program 1");
    EXPECT_FALSE(rack->setProgram(2, error));
}

TEST(PluginRackTest, RejectsParametersBeyondThePreparedChangeQueue) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    PluginRackTestPeer::addParameterAndPublishCount(
        *rack, new juce::AudioParameterFloat("added", "Added", 0.0f, 1.0f, 0.5f));
    ASSERT_EQ(rack->parameterCount(), 1u);

    EXPECT_FALSE(rack->setParameter(0, 0.75f, error));
    EXPECT_EQ(error, "Plugin parameter change queue cannot handle this index.");
    const auto state = rack->persistedState(error);
    ASSERT_TRUE(state.has_value()) << error;
    ASSERT_EQ(state->parameterValues.size(), 1u);
    EXPECT_FLOAT_EQ(state->parameterValues[0], 0.5f);
}

TEST(PluginRackTest, RestoresPersistedPluginState) {
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<StateTestProcessor>(), kSampleRate,
                                            kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

    juce::AudioBuffer<float> output(2, kBlockSize);
    const auto process = [&] {
        rack->process(nullptr, 0, output.getArrayOfWritePointers(), 2, kBlockSize);
    };
    ASSERT_TRUE(rack->setParameter(0, 0.75f, error)) << error;
    process();
    const auto saved = rack->persistedState(error);
    ASSERT_TRUE(saved.has_value()) << error;
    ASSERT_TRUE(rack->setParameter(0, 0.25f, error)) << error;

    ASSERT_TRUE(rack->applyPersistedState(*saved, error)) << error;
    process();
    const auto restored = rack->persistedState(error);
    ASSERT_TRUE(restored.has_value()) << error;
    ASSERT_FALSE(restored->parameterValues.empty());
    EXPECT_NEAR(restored->parameterValues[0], 0.75f, 0.0001f);
}

TEST(PluginRackTest, PassesMidiToInstrumentProcessor) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

    EXPECT_TRUE(rack->isInstrument());
    EXPECT_EQ(rack->status().inputChannels, 0);
    EXPECT_EQ(rack->status().outputChannels, 2);

    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize, &midi);

    ASSERT_EQ(trace.midiMessageCount, 1);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 60);
    EXPECT_EQ(trace.lastMidiMessage.getVelocity(), static_cast<juce::uint8>(102));

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize, &midi);
    ASSERT_EQ(trace.midiMessageCount, 2);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOff());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 60);
}

TEST(PluginRackTest, EnforcesMidiCapacityForTimelineAndQueuedEvents) {
    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;
        ASSERT_TRUE(rack->prepareTimelineMidiCapacity(257, error)) << error;

        juce::MidiBuffer timeline;
        for (int index = 0; index < 257; ++index)
            ASSERT_TRUE(
                timeline.addEvent(juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)),
                                  index % kBlockSize));
        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

        rack->process(nullptr, 0, outputs.data(), 2, kBlockSize, &timeline);

        EXPECT_EQ(trace.midiMessageCount, 257);
        EXPECT_EQ(rack->status().droppedMidiEvents, 0);
    }

    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;

        std::array<std::uint8_t, 257> raw{};
        raw.front() = 0xf0;
        raw.back() = 0xf7;
        juce::MidiBuffer timeline;
        ASSERT_TRUE(
            timeline.addEvent(juce::MidiMessage(raw.data(), static_cast<int>(raw.size())), 0));
        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

        rack->process(nullptr, 0, outputs.data(), 2, kBlockSize, &timeline);

        EXPECT_EQ(trace.midiMessageCount, 0);
        EXPECT_EQ(rack->status().droppedMidiEvents, 1);
    }

    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;

        for (int index = 0; index < 257; ++index) {
            const bool accepted =
                rack->enqueueMidi(juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)));
            EXPECT_EQ(accepted, index < 256);
        }

        EXPECT_EQ(rack->status().droppedMidiEvents, 1);
    }

    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;

        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
        // Live MIDI queued between blocks reaches the instrument in the next one.
        ASSERT_TRUE(rack->enqueueMidi(juce::MidiMessage::noteOn(1, 64, 0.5f)));
        rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

        ASSERT_EQ(trace.midiMessageCount, 1);
        EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
        EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 64);
    }

    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;

        std::array<std::uint8_t, 256> raw{};
        raw.front() = 0xf0;
        raw.back() = 0xf7;
        for (int index = 0; index < 256; ++index)
            ASSERT_TRUE(
                rack->enqueueMidi(juce::MidiMessage(raw.data(), static_cast<int>(raw.size()))));

        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
        rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

        EXPECT_EQ(trace.midiMessageCount, 256);
        EXPECT_EQ(rack->status().droppedMidiEvents, 0);
    }

    {
        InstrumentTrace trace;
        juce::String error;
        auto rack = PluginRackTestPeer::installInstrument(
            std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
        ASSERT_NE(rack, nullptr) << error;
        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

        rack->allNotesOff();
        ASSERT_TRUE(rack->enqueueMidi(juce::MidiMessage::noteOn(1, 60, 0.8f)));
        rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

        ASSERT_EQ(trace.midiMessages.size(), 49u);
        EXPECT_TRUE(trace.midiMessages.front().isController());
        EXPECT_TRUE(trace.midiMessages.back().isNoteOn());
        EXPECT_TRUE(trace.noteHeld);
    }
}

TEST(PluginRackTest, SendsPanicControllersOnEveryMidiChannel) {
    // Arrange
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    rack->allNotesOff();
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    // Assert
    ASSERT_EQ(trace.midiMessages.size(), 48u);
    for (int channel = 1; channel <= 16; ++channel) {
        const auto offset = static_cast<std::size_t>((channel - 1) * 3);
        EXPECT_TRUE(trace.midiMessages[offset].isController());
        EXPECT_EQ(trace.midiMessages[offset].getChannel(), channel);
        EXPECT_EQ(trace.midiMessages[offset].getControllerNumber(), 123);
        EXPECT_EQ(trace.midiMessages[offset].getControllerValue(), 0);

        EXPECT_TRUE(trace.midiMessages[offset + 1].isController());
        EXPECT_EQ(trace.midiMessages[offset + 1].getChannel(), channel);
        EXPECT_EQ(trace.midiMessages[offset + 1].getControllerNumber(), 120);
        EXPECT_EQ(trace.midiMessages[offset + 1].getControllerValue(), 0);

        EXPECT_TRUE(trace.midiMessages[offset + 2].isController());
        EXPECT_EQ(trace.midiMessages[offset + 2].getChannel(), channel);
        EXPECT_EQ(trace.midiMessages[offset + 2].getControllerNumber(), 64);
        EXPECT_EQ(trace.midiMessages[offset + 2].getControllerValue(), 0);
    }
}

TEST(PluginRackTest, BindsStableParameterIdsAndAppliesAutomationBeforeRealVstProcessing) {
    // Arrange
    juce::ScopedJuceInitialiser_GUI initialiser;
    PluginRack rack;
    juce::String error;
    ASSERT_FALSE(rack.load(RIFFRA_TEST_EFFECT_BUNDLE_PATH, kSampleRate, kBlockSize,
                           PluginProcessingMode::offline)
                     .has_value());
    const auto parameters = rack.parameters();
    ASSERT_FALSE(parameters.empty());
    ASSERT_TRUE(parameters[0].parameterId.has_value());
    const auto state = rack.persistedState(error);
    ASSERT_TRUE(state.has_value()) << error;
    AutomationRuntime lane;
    lane.setPoints({{0, 0.25f}, {32, 0.75f}}, true);
    auto binding = rack.bindAutomation(*parameters[0].parameterId, lane, *state, true, error);
    ASSERT_TRUE(binding.has_value()) << error;
    std::array<PluginParameterAutomation, 1> bindings{std::move(*binding)};
    std::array<float, 32> input, left{}, right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act / Assert
    ASSERT_TRUE(rack.setParameter(0, state->parameterValues[0], error));
    rack.process(inputs.data(), 1, outputs.data(), 2, 32, nullptr, bindings, 0, true);
    EXPECT_NEAR(left.back(), 0.0625f, 0.0001f);
    rack.process(inputs.data(), 1, outputs.data(), 2, 32, nullptr, bindings, 32, true);
    EXPECT_NEAR(left.back(), 0.1875f, 0.0001f);
    rack.process(inputs.data(), 1, outputs.data(), 2, 32, nullptr, bindings, 64, false);
    EXPECT_NEAR(rack.parameters()[0].value, state->parameterValues[0], 0.0001f);
    EXPECT_FALSE(rack.bindAutomation("absent-parameter", lane, *state, true, error).has_value());
    const auto discrete =
        std::find_if(parameters.begin(), parameters.end(), [](const auto& parameter) {
            return parameter.discrete && parameter.parameterId.has_value();
        });
    ASSERT_NE(discrete, parameters.end());
    EXPECT_FALSE(
        rack.bindAutomation(*discrete->parameterId, lane, *state, false, error).has_value());
    EXPECT_TRUE(rack.bindAutomation(*discrete->parameterId, lane, *state, true, error).has_value());
}

}  // namespace riffra
