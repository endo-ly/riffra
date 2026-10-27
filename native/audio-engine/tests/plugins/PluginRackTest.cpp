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
    rack->process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.5f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.5f);
}

TEST(PluginRackTest, SendsTransportAndStoppedBlockToProcessor) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> left{}, right{};
    const std::array<float*, 2> outputs{left.data(), right.data()};
    PluginTransportPosition position{48'000, 48'000.0, 2.0,   90.0, 0.0, 3,
                                     4,      true,     false, true, 0.0, 6.0};
    rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, nullptr, {},
                  position.samplePosition, true, &position);
    ASSERT_TRUE(trace.position.hasValue());
    EXPECT_EQ(*trace.position->getTimeInSamples(), 48'000);
    EXPECT_DOUBLE_EQ(*trace.position->getPpqPosition(), 2.0);
    EXPECT_DOUBLE_EQ(*trace.position->getBpm(), 90.0);
    EXPECT_EQ(trace.position->getTimeSignature()->numerator, 3);
    EXPECT_TRUE(trace.position->getIsPlaying());
    position.playing = false;
    rack->notifyTransportStopped(position, kBlockSize);
    ASSERT_TRUE(trace.position.hasValue());
    EXPECT_FALSE(trace.position->getIsPlaying());
    const auto processedBeforeBypass = trace.processBlockCount;
    rack->setBypassed(true);
    rack->notifyTransportStopped(position, kBlockSize);
    EXPECT_EQ(trace.processBlockCount, processedBeforeBypass);
}

TEST(PluginRackTest, ReturnsDrySignalWhenBypassedOrUnloaded) {
    const auto expectDrySignal = [](PluginRack& rack) {
        std::array<float, kBlockSize> input{};
        std::array<float, kBlockSize> outputLeft{};
        std::array<float, kBlockSize> outputRight{};
        input.fill(0.25f);
        const std::array<const float*, 1> inputs{input.data()};
        const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
        rack.process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, kBlockSize);

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
        rack->process(nullptr, 0, nullptr, 0, output.getArrayOfWritePointers(), 2, kBlockSize);
    };
    ASSERT_TRUE(rack->setParameter(0, 0.75f, error)) << error;
    process();
    const auto saved = rack->persistedState(error);
    ASSERT_TRUE(saved.has_value()) << error;
    auto opaqueState = *saved;
    opaqueState.parameterValues.clear();

    // The saved snapshot and its opaque VST3 state alone each restore the plugin itself.
    for (const auto& state : {*saved, opaqueState}) {
        ASSERT_TRUE(rack->setParameter(0, 0.25f, error)) << error;
        process();
        ASSERT_TRUE(rack->applyPersistedState(state, error)) << error;
        process();
        const auto restored = rack->parameters();
        ASSERT_FALSE(restored.empty());
        EXPECT_NEAR(restored[0].value, 0.75f, 0.0001f);
    }
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
    rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, &midi);

    ASSERT_EQ(trace.midiMessageCount, 1);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 60);
    EXPECT_EQ(trace.lastMidiMessage.getVelocity(), static_cast<juce::uint8>(102));

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, &midi);
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

        rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, &timeline);

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

        rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize, &timeline);

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
        rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize);

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
        rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize);

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
        rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize);

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
    rack->process(nullptr, 0, nullptr, 0, outputs.data(), 2, kBlockSize);

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
                           PluginProcessingMode::offline, false)
                     .has_value());
    const auto parameters = rack.parameters();
    ASSERT_FALSE(parameters.empty());
    ASSERT_TRUE(parameters[0].parameterId.has_value());
    const auto state = rack.persistedState(error);
    ASSERT_TRUE(state.has_value()) << error;
    ASSERT_TRUE(rack.applyPersistedState(*state, error)) << error;
    AutomationRuntime lane;
    lane.setPoints({{0, 0.25f}, {32, 0.75f}}, true);
    AutomationBindingFailure failure = AutomationBindingFailure::invalidParameter;
    auto binding =
        rack.bindAutomation(*parameters[0].parameterId, lane, *state, true, failure, error);
    ASSERT_TRUE(binding.has_value()) << error;
    EXPECT_FLOAT_EQ(binding->staticValue, state->parameterValues[0]);
    std::array<PluginParameterAutomation, 1> bindings{std::move(*binding)};
    std::array<float, 32> input, left{}, right{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{left.data(), right.data()};

    // Act / Assert
    ASSERT_TRUE(rack.setParameter(0, state->parameterValues[0], error)) << error;
    rack.process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, 32, nullptr, bindings, 0, true);
    EXPECT_NEAR(left.back(), 0.0625f, 0.0001f);
    rack.process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, 32, nullptr, bindings, 32, true);
    EXPECT_NEAR(left.back(), 0.1875f, 0.0001f);
    EXPECT_FALSE(rack.setParameter(0, 0.9f, error));
    EXPECT_FALSE(rack.applyPersistedState(*state, error));
    EXPECT_FALSE(rack.setProgram(0, error));
    rack.process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, 32, nullptr, {}, 48, true);
    const auto restoredState = rack.persistedState(error);
    ASSERT_TRUE(restoredState.has_value()) << error;
    EXPECT_NEAR(restoredState->parameterValues[0], state->parameterValues[0], 0.0001f);
    EXPECT_NEAR(rack.parameters()[0].value, state->parameterValues[0], 0.0001f);
    rack.process(inputs.data(), 1, nullptr, 0, outputs.data(), 2, 32, nullptr, bindings, 64, false);
    EXPECT_NEAR(rack.parameters()[0].value, state->parameterValues[0], 0.0001f);
    EXPECT_FALSE(
        rack.bindAutomation("absent-parameter", lane, *state, true, failure, error).has_value());
    EXPECT_EQ(failure, AutomationBindingFailure::laneUnavailable);
    EXPECT_EQ(error, "automation parameter ID is not present: absent-parameter");
    const auto discrete =
        std::find_if(parameters.begin(), parameters.end(), [](const auto& parameter) {
            return parameter.discrete && parameter.parameterId.has_value();
        });
    ASSERT_NE(discrete, parameters.end());
    EXPECT_FALSE(rack.bindAutomation(*discrete->parameterId, lane, *state, false, failure, error)
                     .has_value());
    EXPECT_EQ(failure, AutomationBindingFailure::laneUnavailable);
    EXPECT_TRUE(rack.bindAutomation(*discrete->parameterId, lane, *state, true, failure, error)
                    .has_value());
}

TEST(PluginRackTest, AutomationBindingUsesProjectedStateWithoutCapturingPluginState) {
    // Arrange
    ProcessorTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace, true),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;
    const auto parameters = rack->parameters();
    ASSERT_EQ(parameters.size(), 1u);
    ASSERT_TRUE(parameters[0].parameterId.has_value());
    PluginStateSpec projected;
    projected.parameterValues = {0.4f};
    ASSERT_TRUE(rack->applyPersistedState(projected, error)) << error;
    const auto retained = rack->persistedState(error);
    ASSERT_TRUE(retained.has_value()) << error;
    EXPECT_FLOAT_EQ(retained->parameterValues[0], projected.parameterValues[0]);
    EXPECT_EQ(trace.stateCaptureCount, 0);
    AutomationRuntime lane;
    lane.setPoints({{0, 0.25f}, {32, 0.75f}}, true);

    // Act
    AutomationBindingFailure failure = AutomationBindingFailure::invalidParameter;
    const auto binding =
        rack->bindAutomation(*parameters[0].parameterId, lane, projected, true, failure, error);

    // Assert
    ASSERT_TRUE(binding.has_value()) << error;
    EXPECT_FLOAT_EQ(binding->staticValue, projected.parameterValues[0]);
    EXPECT_EQ(trace.stateCaptureCount, 0);

    // An installed plugin may retain an old lane whose parameter is no longer automatable.
    PluginRackTestPeer::addParameterAndPublishCount(
        *rack, new juce::AudioParameterFloat(
                   juce::ParameterID{"test.manual", 1}, "Manual Parameter",
                   juce::NormalisableRange<float>{0.0f, 1.0f}, 0.5f,
                   juce::AudioParameterFloatAttributes().withAutomatable(false)));
    const auto unavailable =
        rack->bindAutomation("test.manual", lane, projected, true, failure, error);
    EXPECT_FALSE(unavailable.has_value());
    EXPECT_EQ(failure, AutomationBindingFailure::laneUnavailable);
    EXPECT_EQ(error,
              "automation parameter is not automatable with this interpolation: test.manual");

    // A newly duplicated stable ID must never bind to its first matching parameter.
    PluginRackTestPeer::addParameterAndPublishCount(
        *rack,
        new juce::AudioParameterFloat(juce::ParameterID{"test.parameter", 1}, "Duplicate Parameter",
                                      juce::NormalisableRange<float>{0.0f, 1.0f}, 0.25f));
    const auto parametersAfterDuplicate = rack->parameters();
    ASSERT_EQ(parametersAfterDuplicate.size(), 3u);
    EXPECT_FALSE(parametersAfterDuplicate[0].parameterId.has_value());
    EXPECT_FALSE(parametersAfterDuplicate[2].parameterId.has_value());
    const auto ambiguous =
        rack->bindAutomation("test.parameter", lane, projected, true, failure, error);
    EXPECT_FALSE(ambiguous.has_value());
    EXPECT_EQ(failure, AutomationBindingFailure::laneUnavailable);
    EXPECT_EQ(error, "automation parameter ID is ambiguous: test.parameter");
    EXPECT_FALSE(rack->setAutomationPreview("test.parameter", 7, 0.5f, error));
    EXPECT_EQ(error, "automation parameter ID is ambiguous: test.parameter");
}

TEST(PluginRackTest, ReportsAuxiliaryInputCapabilityWithoutEnablingIt) {
    // Arrange
    SidechainTrace trace;
    juce::String error;

    // Act
    auto rack =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(true, true, trace),
                                    kSampleRate, kBlockSize, error);

    // Assert
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    EXPECT_TRUE(status.sidechain.available);
    EXPECT_TRUE(status.sidechain.mono);
    EXPECT_TRUE(status.sidechain.stereo);
    EXPECT_EQ(status.sidechain.channels, 0);
    EXPECT_FALSE(status.sidechain.reason.has_value());

    std::array<float, kBlockSize> left{};
    std::array<float, kBlockSize> right{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    left.fill(0.25f);
    right.fill(-0.25f);
    const std::array<const float*, 2> inputs{left.data(), right.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(inputs.data(), 2, nullptr, 0, outputs.data(), 2, kBlockSize);
    EXPECT_EQ(trace.bufferChannels, 2);
    EXPECT_EQ(trace.auxBusChannels, 0);
    EXPECT_FLOAT_EQ(outputLeft[0], 0.25f);
    EXPECT_FLOAT_EQ(outputRight[0], -0.25f);
}

TEST(PluginRackTest, ProcessesStereoAuxiliaryInputAtItsBufferOffsets) {
    // Arrange
    SidechainTrace trace;
    juce::String error;
    auto rack =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(true, true, trace),
                                    kSampleRate, kBlockSize, error, false, true);
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    EXPECT_EQ(status.sidechain.channels, 2);
    EXPECT_FALSE(status.sidechain.reason.has_value());

    std::array<float, kBlockSize> left{};
    std::array<float, kBlockSize> right{};
    std::array<float, kBlockSize> auxLeft{};
    std::array<float, kBlockSize> auxRight{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    left.fill(0.25f);
    right.fill(-0.25f);
    auxLeft.fill(0.5f);
    auxRight.fill(0.125f);
    const std::array<const float*, 2> inputs{left.data(), right.data()};
    const std::array<const float*, 2> aux{auxLeft.data(), auxRight.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    rack->process(inputs.data(), 2, aux.data(), 2, outputs.data(), 2, kBlockSize);

    // Assert
    EXPECT_EQ(trace.bufferChannels, 4);
    EXPECT_EQ(trace.auxBusChannels, 2);
    EXPECT_FLOAT_EQ(trace.auxPeak, 0.5f);
    EXPECT_FLOAT_EQ(outputLeft[0], 0.75f);
    EXPECT_FLOAT_EQ(outputRight[0], -0.125f);
}

TEST(PluginRackTest, FoldsStereoAuxiliaryAudioIntoAMonoBus) {
    // Arrange
    SidechainTrace trace;
    juce::String error;
    auto rack =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(true, false, trace),
                                    kSampleRate, kBlockSize, error, false, true);
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    EXPECT_TRUE(status.sidechain.mono);
    EXPECT_FALSE(status.sidechain.stereo);
    EXPECT_EQ(status.sidechain.channels, 1);

    std::array<float, kBlockSize> left{};
    std::array<float, kBlockSize> right{};
    std::array<float, kBlockSize> auxLeft{};
    std::array<float, kBlockSize> auxRight{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    left.fill(0.4f);
    right.fill(0.2f);
    auxLeft.fill(1.0f);
    auxRight.fill(0.2f);
    const std::array<const float*, 2> inputs{left.data(), right.data()};
    const std::array<const float*, 2> aux{auxLeft.data(), auxRight.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    rack->process(inputs.data(), 2, aux.data(), 2, outputs.data(), 2, kBlockSize);

    // Assert
    EXPECT_EQ(trace.bufferChannels, 3);
    EXPECT_EQ(trace.auxBusChannels, 1);
    EXPECT_FLOAT_EQ(trace.auxPeak, 0.6f);
    EXPECT_FLOAT_EQ(outputLeft[0], 1.0f);
    EXPECT_FLOAT_EQ(outputRight[0], 0.8f);
}

TEST(PluginRackTest, FeedsSilenceWhenNoSidechainAudioIsConnected) {
    // Arrange
    SidechainTrace trace;
    juce::String error;
    auto rack =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(true, true, trace),
                                    kSampleRate, kBlockSize, error, false, true);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> left{};
    std::array<float, kBlockSize> right{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    left.fill(0.25f);
    right.fill(-0.25f);
    const std::array<const float*, 2> inputs{left.data(), right.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    // Act
    rack->process(inputs.data(), 2, nullptr, 0, outputs.data(), 2, kBlockSize);

    // Assert
    EXPECT_EQ(trace.auxBusChannels, 2);
    EXPECT_FLOAT_EQ(trace.auxPeak, 0.0f);
    EXPECT_FLOAT_EQ(outputLeft[0], 0.25f);
    EXPECT_FLOAT_EQ(outputRight[0], -0.25f);
}

TEST(PluginRackTest, KeepsAnEffectUsableWithoutAnAuxiliaryInputBus) {
    // Arrange
    ProcessorTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), kSampleRate,
                                            kBlockSize, error);

    // Act / Assert
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    EXPECT_FALSE(status.sidechain.available);
    EXPECT_EQ(status.sidechain.channels, 0);
    ASSERT_TRUE(status.sidechain.reason.has_value());
    EXPECT_EQ(*status.sidechain.reason, "The VST3 does not expose an auxiliary input bus.");
    const auto prepared = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace),
                                                      kSampleRate, kBlockSize, error, false, true);
    ASSERT_NE(prepared, nullptr) << error;
    EXPECT_FALSE(prepared->status().sidechain.available);
}

TEST(PluginRackTest, KeepsAnEffectUsableWhenItsSidechainLayoutIsUnsupported) {
    // Arrange
    SidechainTrace trace;
    juce::String error;
    auto rack =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(false, false, trace),
                                    kSampleRate, kBlockSize, error);

    // Act / Assert
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    EXPECT_TRUE(status.sidechain.available);
    EXPECT_FALSE(status.sidechain.mono);
    EXPECT_FALSE(status.sidechain.stereo);
    ASSERT_TRUE(status.sidechain.reason.has_value());
    EXPECT_EQ(*status.sidechain.reason,
              "The VST3 does not support a mono or stereo auxiliary input.");
    const auto prepared =
        PluginRackTestPeer::install(std::make_unique<TestSidechainProcessor>(false, false, trace),
                                    kSampleRate, kBlockSize, error, false, true);
    ASSERT_NE(prepared, nullptr) << error;
    EXPECT_EQ(prepared->status().sidechain.channels, 0);
}

}  // namespace riffra
