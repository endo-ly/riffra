#include <gtest/gtest.h>

#include <atomic>
#include <thread>
#include <vector>

#include "../support/TestAudioProcessor.h"
#include "audio/PluginAudition.h"

namespace riffra {
namespace {

constexpr double kSampleRate = 48'000.0;
constexpr int kBlockSize = 256;

std::unique_ptr<PluginRack> instrumentRack(InstrumentTrace& trace) {
    juce::String error;
    auto rack = PluginRackTestPeer::installInstrument(
        std::make_unique<TestInstrumentProcessor>(trace), kSampleRate, kBlockSize, error);
    EXPECT_NE(rack, nullptr) << error;
    return rack;
}

/// An effect that doubles its input.
std::unique_ptr<PluginRack> effectRack(ProcessorTrace& trace) {
    juce::String error;
    auto rack = PluginRackTestPeer::install(std::make_unique<TestProcessor>(trace), kSampleRate,
                                            kBlockSize, error);
    EXPECT_NE(rack, nullptr) << error;
    return rack;
}

juce::AudioBuffer<float> mixOnce(PluginAudition& audition, const float inputLevel,
                                 const double sampleRate = kSampleRate) {
    std::vector<float> input(kBlockSize, inputLevel);
    juce::AudioBuffer<float> output(2, kBlockSize);
    output.clear();
    audition.mix(input.data(), output.getArrayOfWritePointers(), 2, kBlockSize, sampleRate);
    return output;
}

}  // namespace

TEST(PluginAuditionTest, PlaysAnInstrumentFromLiveMidiOnlyWhileInstalled) {
    // Arrange
    PluginAudition audition;
    InstrumentTrace trace;
    const auto note = juce::MidiMessage::noteOn(1, 60, 0.8f);
    const auto routedBeforeInstall = audition.enqueueMidi(note);

    // Act
    audition.install(instrumentRack(trace), kSampleRate, kBlockSize);
    const auto routedWhileInstalled = audition.enqueueMidi(note);
    const auto monitorsInput = audition.monitorsInput();
    const auto played = mixOnce(audition, 0.1f);
    const auto otherFormat = mixOnce(audition, 0.1f, 44'100.0);
    const auto returned = audition.uninstall();

    // Assert
    EXPECT_FALSE(routedBeforeInstall);
    EXPECT_TRUE(routedWhileInstalled);
    EXPECT_FALSE(monitorsInput);
    EXPECT_FLOAT_EQ(played.getSample(0, 0), 0.25f);
    EXPECT_FLOAT_EQ(otherFormat.getMagnitude(0, kBlockSize), 0.0f);
    EXPECT_NE(returned, nullptr);
    EXPECT_EQ(audition.installed(), nullptr);
    EXPECT_FALSE(audition.enqueueMidi(note));
    EXPECT_FLOAT_EQ(mixOnce(audition, 0.1f).getMagnitude(0, kBlockSize), 0.0f);
}

TEST(PluginAuditionTest, RunsTheAudioInputThroughAnEffectAndLeavesLiveMidiToTracks) {
    // Arrange
    PluginAudition audition;
    ProcessorTrace trace;
    audition.install(effectRack(trace), kSampleRate, kBlockSize);

    // Act
    const auto routedMidi = audition.enqueueMidi(juce::MidiMessage::noteOn(1, 60, 0.8f));
    const auto monitorsInput = audition.monitorsInput();
    const auto output = mixOnce(audition, 0.1f);

    // Assert
    EXPECT_FALSE(routedMidi);
    EXPECT_TRUE(monitorsInput);
    EXPECT_FLOAT_EQ(output.getSample(0, 0), 0.2f);
    EXPECT_FLOAT_EQ(output.getSample(1, kBlockSize - 1), 0.2f);
}

TEST(PluginAuditionTest, FadesTheLastBlockToSilenceWhenUninstalledDuringPlayback) {
    // Arrange
    PluginAudition audition;
    ProcessorTrace trace;
    audition.install(effectRack(trace), kSampleRate, kBlockSize);
    std::atomic<bool> uninstalled{false};
    juce::AudioBuffer<float> lastAudible(2, kBlockSize);
    lastAudible.clear();

    // Act
    std::thread control([&] {
        (void)audition.uninstall();
        uninstalled.store(true);
    });
    while (!uninstalled.load()) {
        const auto output = mixOnce(audition, 0.25f);
        if (output.getMagnitude(0, 0, kBlockSize) > 0.0f) lastAudible.makeCopyOf(output);
    }
    control.join();

    // Assert
    EXPECT_GT(lastAudible.getSample(0, 0), 0.4f);
    EXPECT_FLOAT_EQ(lastAudible.getSample(0, kBlockSize - 1), 0.0f);
    EXPECT_FALSE(audition.monitorsInput());
}

}  // namespace riffra
