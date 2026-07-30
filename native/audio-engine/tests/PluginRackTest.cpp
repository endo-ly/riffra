#include "TestAudioProcessor.h"

#include <gtest/gtest.h>

#include <array>

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 32;

std::unique_ptr<PluginRack> makeRack(
    ProcessorTrace& trace,
    juce::String& error) {
    return PluginRackTestPeer::install(
        std::make_unique<TestProcessor>(trace), kSampleRate, kBlockSize, error);
}

} // namespace

TEST(PluginRackTest, ConfiguresStereoProcessor)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("inputChannels", -1)), 2);
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("outputChannels", -1)), 2);
}

TEST(PluginRackTest, PreparesProcessorBeforeProcessing)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_TRUE(trace.prepared);
    EXPECT_TRUE(trace.processed);
}

TEST(PluginRackTest, ProcessesMonoInputToStereoOutput)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.5f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.5f);
}

TEST(PluginRackTest, ReturnsDrySignalWhenBypassed)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    rack->setBypassed(true);

    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack->process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.25f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.25f);
}

TEST(PluginRackTest, ReturnsDrySignalWhenNoProcessorIsLoaded)
{
    PluginRack rack;
    std::array<float, kBlockSize> input {};
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    input.fill(0.25f);
    const std::array<const float*, 1> inputs { input.data() };
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack.process(inputs.data(), 1, outputs.data(), 2, kBlockSize);

    EXPECT_FLOAT_EQ(outputLeft.front(), 0.25f);
    EXPECT_FLOAT_EQ(outputRight.back(), 0.25f);
}

TEST(PluginRackTest, ReportsProcessedBlockCount)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    EXPECT_EQ(static_cast<juce::int64>(rack->status().getProperty("processedBlocks", 0)), 1);
}

TEST(PluginRackTest, DoesNotExposePersistedStateInRuntimeStatus)
{
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

TEST(PluginRackTest, ReleasesProcessorWhenCleared)
{
    ProcessorTrace trace;
    juce::String error;
    auto rack = makeRack(trace, error);
    ASSERT_NE(rack, nullptr) << error;
    rack->clear();

    EXPECT_TRUE(trace.released);
    EXPECT_FALSE(rack->isLoaded());
}

TEST(PluginRackTest, ConfiguresInstrumentWithoutInputBus)
{
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(
        std::make_unique<TestInstrumentProcessor>(trace),
        kSampleRate,
        kBlockSize,
        error);
    ASSERT_NE(rack, nullptr) << error;

    EXPECT_TRUE(rack->isInstrument());
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("inputChannels", -1)), 0);
    EXPECT_EQ(static_cast<int>(rack->status().getProperty("outputChannels", -1)), 2);
}

TEST(PluginRackTest, PassesMidiToInstrumentProcessor)
{
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(
        std::make_unique<TestInstrumentProcessor>(trace),
        kSampleRate,
        kBlockSize,
        error);
    ASSERT_NE(rack, nullptr) << error;

    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
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

TEST(PluginRackTest, DrainsQueuedLiveMidiIntoTheNextBlock)
{
    // Arrange
    InstrumentTrace trace;
    juce::String error;
    auto rack = PluginRackTestPeer::install(
        std::make_unique<TestInstrumentProcessor>(trace),
        kSampleRate,
        kBlockSize,
        error);
    ASSERT_NE(rack, nullptr) << error;
    std::array<float, kBlockSize> outputLeft {};
    std::array<float, kBlockSize> outputRight {};
    const std::array<float*, 2> outputs { outputLeft.data(), outputRight.data() };
    rack->enqueueMidi(juce::MidiMessage::noteOn(1, 64, 0.5f));

    // Act
    rack->process(nullptr, 0, outputs.data(), 2, kBlockSize);

    // Assert
    ASSERT_EQ(trace.midiMessageCount, 1);
    EXPECT_TRUE(trace.lastMidiMessage.isNoteOn());
    EXPECT_EQ(trace.lastMidiMessage.getNoteNumber(), 64);
}

} // namespace riffra
