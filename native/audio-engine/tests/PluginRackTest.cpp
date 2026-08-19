#include <gtest/gtest.h>

#include <array>

#include "TestAudioProcessor.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

std::unique_ptr<PluginRack> makeRack(ProcessorTrace& trace, juce::String& error) {
    return PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), kSampleRate,
                                       kBlockSize, error);
}

}  // namespace

TEST(PluginRackTest, ConfiguresStereoProcessor) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("inputChannels", -1)), 2);
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("outputChannels", -1)), 2);
}

TEST(PluginRackTest, PreparesProcessorBeforeProcessing) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_TRUE(trace.prepared);
    EXPECT_TRUE(trace.processed);
}

TEST(PluginRackTest, RepreparesProcessorForTheCurrentAudioDeviceFormat) {
    // Arrange
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    // Act
    rack->prepare(44'100.0, 1024);

    // Assert
    EXPECT_DOUBLE_EQ(trace.sampleRate, 44'100.0);
    EXPECT_EQ(trace.blockSize, 1024);
    EXPECT_DOUBLE_EQ(static_cast<double>(rack->status().getProperty("sampleRate", 0.0)), 44'100.0);
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("blockSize", 0)), 1024);
}

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

TEST(PluginRackTest, ReturnsDrySignalWhenBypassed) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    rack->setBypassed(true);

    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.25f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.25f);
}

TEST(PluginRackTest, ReturnsDrySignalWhenNoProcessorIsLoaded) {
    PluginRack rack;
    std::array<float, kBlockSize> input{};
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs{input.data()};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack.process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.25f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.25f);
}

TEST(PluginRackTest, ReportsProcessedBlockCount) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(static_cast<juce::int64>(rack->status().getProperty("processedBlocks", 0)), 1);
}

TEST(PluginRackTest, DoesNotExposePersistedStateInRuntimeStatus) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    const auto status = rack->status();
    const auto parameterStatus = rack->parameterStatus();

    EXPECT_FALSE(status.hasProperty("stateData"));
    EXPECT_FALSE(status.hasProperty("parameters"));
    EXPECT_TRUE(parameterStatus.hasProperty("parameters"));
    EXPECT_FALSE(parameterStatus.hasProperty("stateData"));
}

TEST(PluginRackTest, ReleasesProcessorWhenCleared) {
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    rack->clear();

    EXPECT_TRUE(trace.released);
    EXPECT_FALSE(rack->isLoaded());
}

TEST(PluginRackTest, ConfiguresInstrumentWithoutInputBus) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

    EXPECT_TRUE(rack->isInstrument());
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("inputChannels", -1)), 0);
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("outputChannels", -1)), 2);
}

TEST(PluginRackTest, PassesMidiToInstrumentProcessor) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

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

TEST(PluginRackTest, DrainsQueuedLiveMidiIntoTheNextBlock) {
    // Arrange
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->enqueueMidi(juce::MidiMessage::noteOn(1, 64, 0.5f));

    // Act
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    // Assert
    ASSERT_EQ(trace.midiMessageCount, 1);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 64);
}

TEST(PluginRackTest, ReportsQueuedMidiOverflow) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

    for (int index = 0; index < 257; ++index)
        rack->enqueueMidi(juce::MidiMessage::noteOn(1, 60, static_cast<juce::uint8>(100)));

    EXPECT_EQ(static_cast<juce::int64>(rack->status().getProperty("droppedMidiEvents", -1)), 1);
}

TEST(PluginRackTest, DeliversMaximumSizedQueuedMidiPacket) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<std::uint8_t, 256> raw{};
    raw.front() = 0xf0;
    raw.back() = 0xf7;
    for (int index = 0; index < 256; ++index)
        rack->enqueueMidi(juce::MidiMessage(raw.data(), static_cast<int>(raw.size())));

    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(trace.midiMessageCount, 256);
    EXPECT_EQ(static_cast<juce::int64>(rack->status().getProperty("droppedMidiEvents", -1)), 0);
}

TEST(PluginRackTest, SendsPanicControllersOnEveryMidiChannel) {
    // Arrange
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
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

TEST(PluginRackTest, DeliversQueuedNoteAfterResetControllers) {
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestInstrumentProcessor>(trace),
                                            kSampleRate, kBlockSize, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft{};
    std::array<float, kBlockSize> outputRight{};
    const std::array<float*, 2> outputs{outputLeft.data(), outputRight.data()};

    rack->allNotesOff();
    rack->enqueueMidi(juce::MidiMessage::noteOn(1, 60, 0.8f));
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    ASSERT_EQ(trace.midiMessages.size(), 49u);
    EXPECT_TRUE(trace.midiMessages.front().isController());
    EXPECT_TRUE(trace.midiMessages.back().isNoteOn());
    EXPECT_TRUE(trace.noteHeld);
}

}  // namespace riffra
