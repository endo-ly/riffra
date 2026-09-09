#include <gtest/gtest.h>

#include <chrono>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

#include "timeline/instruments/SonalloyInstrumentRuntime.h"

namespace riffra {
namespace {

using Clock = std::chrono::steady_clock;

struct BenchmarkTrack final {
    std::unique_ptr<SonalloyInstrumentRuntime> runtime;
    juce::AudioBuffer<float> output{2, 256};
};

struct BenchmarkResult final {
    double callbackMicrosecondsPerBlock = 0.0;
    double trackMicrosecondsPerBlock = 0.0;
    double sonalloyMicrosecondsPerBlock = 0.0;
};

std::unique_ptr<SonalloyInstrumentRuntime> loadBenchmarkRuntime(juce::String& error) {
    const juce::File presetDirectory(RIFFRA_SONALLOY_TEST_PRESET_ROOT);
    const auto directory = presetDirectory.getChildFile("01-clean-sub-bass");
    const auto definition = directory.getChildFile("definition.json");
    return SonalloyInstrumentRuntime::create(definition.loadFileAsString(),
                                             directory.getFullPathName(), 48'000.0, 256, error);
}

void processBenchmarkBlock(BenchmarkTrack& track, const juce::MidiBuffer* midi,
                           std::uint64_t& trackNanoseconds, std::uint64_t& sonalloyNanoseconds) {
    const auto trackStart = Clock::now();
    track.output.clear();
    const auto sonalloyStart = Clock::now();
    InstrumentProcessContext context;
    context.playing = true;
    track.runtime->process(track.output.getArrayOfWritePointers(), track.output.getNumChannels(),
                           track.output.getNumSamples(), midi, context);
    const auto sonalloyEnd = Clock::now();
    const auto trackEnd = Clock::now();
    sonalloyNanoseconds += static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(sonalloyEnd - sonalloyStart).count());
    trackNanoseconds += static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(trackEnd - trackStart).count());
}

BenchmarkResult runBenchmark(const int trackCount) {
    constexpr int warmupBlocks = 16;
    constexpr int measuredBlocks = 64;
    std::vector<BenchmarkTrack> tracks;
    tracks.reserve(static_cast<std::size_t>(trackCount));
    for (int index = 0; index < trackCount; ++index) {
        juce::String error;
        auto runtime = loadBenchmarkRuntime(error);
        EXPECT_NE(runtime, nullptr) << error.toStdString();
        if (runtime == nullptr) continue;
        tracks.push_back(BenchmarkTrack{std::move(runtime)});
    }
    EXPECT_EQ(static_cast<int>(tracks.size()), trackCount);
    if (static_cast<int>(tracks.size()) != trackCount) return {};

    juce::MidiBuffer noteOn;
    noteOn.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    std::uint64_t warmupTrackNanoseconds = 0;
    std::uint64_t warmupSonalloyNanoseconds = 0;
    for (int block = 0; block < warmupBlocks; ++block) {
        const auto* midi = block == 0 ? &noteOn : nullptr;
        for (auto& track : tracks)
            processBenchmarkBlock(track, midi, warmupTrackNanoseconds, warmupSonalloyNanoseconds);
    }

    std::uint64_t callbackNanoseconds = 0;
    std::uint64_t trackNanoseconds = 0;
    std::uint64_t sonalloyNanoseconds = 0;
    for (int block = 0; block < measuredBlocks; ++block) {
        const auto callbackStart = Clock::now();
        for (auto& track : tracks)
            processBenchmarkBlock(track, nullptr, trackNanoseconds, sonalloyNanoseconds);
        const auto callbackEnd = Clock::now();
        callbackNanoseconds += static_cast<std::uint64_t>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(callbackEnd - callbackStart)
                .count());
    }

    for (const auto& track : tracks) {
        EXPECT_EQ(track.runtime->faultCode(), 0u);
        EXPECT_EQ(track.runtime->droppedMidiEvents(), 0u);
    }

    const auto blockCount = static_cast<double>(measuredBlocks);
    return {
        static_cast<double>(callbackNanoseconds) / blockCount / 1'000.0,
        static_cast<double>(trackNanoseconds) / blockCount / 1'000.0,
        static_cast<double>(sonalloyNanoseconds) / blockCount / 1'000.0,
    };
}

}  // namespace

TEST(SonalloyInstrumentRuntimeBenchmarkTest, ReportsScalingForOneToEightTracks) {
    for (const int trackCount : {1, 2, 4, 8}) {
        const auto result = runBenchmark(trackCount);
        std::cout << std::fixed << std::setprecision(2)
                  << "sonalloy_benchmark tracks=" << trackCount
                  << " callback_us_per_block=" << result.callbackMicrosecondsPerBlock
                  << " track_us_per_block=" << result.trackMicrosecondsPerBlock
                  << " sonalloy_us_per_block=" << result.sonalloyMicrosecondsPerBlock << '\n';
    }
}

}  // namespace riffra
