#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

#if defined(_MSC_VER) && defined(_DEBUG)
#include <crtdbg.h>
#endif

#if defined(RIFFRA_TRACK_MALLOC)
namespace riffra::test {
std::atomic<std::uint64_t>* trackedMallocCounter = nullptr;
}

extern "C" void* __real_malloc(std::size_t size);

extern "C" void* __wrap_malloc(const std::size_t size) noexcept {
    if (riffra::test::trackedMallocCounter != nullptr)
        riffra::test::trackedMallocCounter->fetch_add(1, std::memory_order_relaxed);
    return __real_malloc(size);
}
#endif

#include "instrument/SonalloyInstrumentRuntime.h"

namespace riffra {
namespace {

class ScopedAllocationCounter final {
public:
    ScopedAllocationCounter() noexcept {
#if defined(RIFFRA_TRACK_MALLOC)
        previousMallocCounter = test::trackedMallocCounter;
        test::trackedMallocCounter = &allocations;
#elif defined(_MSC_VER) && defined(_DEBUG)
        activeCrtCounter = &allocations;
        previousCrtHook = _CrtSetAllocHook(&allocationHook);
#endif
    }

    ~ScopedAllocationCounter() {
#if defined(RIFFRA_TRACK_MALLOC)
        test::trackedMallocCounter = previousMallocCounter;
#elif defined(_MSC_VER) && defined(_DEBUG)
        activeCrtCounter = nullptr;
        _CrtSetAllocHook(previousCrtHook);
#endif
    }

    [[nodiscard]] std::uint64_t count() const noexcept {
        return allocations.load(std::memory_order_relaxed);
    }

private:
#if defined(_MSC_VER) && defined(_DEBUG)
    static int __cdecl allocationHook(const int allocationType, void*, const std::size_t, const int,
                                      const long, const unsigned char*, const int) {
        if (activeCrtCounter != nullptr &&
            (allocationType == _HOOK_ALLOC || allocationType == _HOOK_REALLOC))
            activeCrtCounter->fetch_add(1, std::memory_order_relaxed);
        return 1;
    }

    static inline std::atomic<std::uint64_t>* activeCrtCounter = nullptr;
    _CRT_ALLOC_HOOK previousCrtHook = nullptr;
#endif

    std::atomic<std::uint64_t> allocations{0};
#if defined(RIFFRA_TRACK_MALLOC)
    std::atomic<std::uint64_t>* previousMallocCounter = nullptr;
#endif
};

juce::File presetRoot() { return juce::File(RIFFRA_SONALLOY_TEST_PRESET_ROOT); }

juce::File sourceRoot() { return juce::File(RIFFRA_SONALLOY_TEST_SOURCE_ROOT); }

std::unique_ptr<SonalloyInstrumentRuntime> loadPreset(const juce::File& directory,
                                                      juce::String& error) {
    const auto definition = directory.getChildFile("definition.json");
    return SonalloyInstrumentRuntime::create(definition.loadFileAsString(),
                                             directory.getFullPathName(), 48'000.0, 256, error);
}

InstrumentProcessContext playingContext() {
    InstrumentProcessContext context;
    context.playing = true;
    return context;
}

void expectFinite(const juce::AudioBuffer<float>& output) {
    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        for (int sample = 0; sample < output.getNumSamples(); ++sample)
            EXPECT_TRUE(std::isfinite(output.getSample(channel, sample)));
}

float maximumMagnitude(const juce::AudioBuffer<float>& output) {
    float magnitude = 0.0f;
    for (int channel = 0; channel < output.getNumChannels(); ++channel)
        magnitude = std::max(magnitude, output.getMagnitude(channel, 0, output.getNumSamples()));
    return magnitude;
}

void processBlock(SonalloyInstrumentRuntime& runtime, juce::AudioBuffer<float>& output,
                  const juce::MidiBuffer* midi = nullptr,
                  const InstrumentProcessContext& context = playingContext()) {
    output.clear();
    runtime.process(output.getArrayOfWritePointers(), output.getNumChannels(),
                    output.getNumSamples(), midi, context);
}

}  // namespace

TEST(SonalloyInstrumentRuntimeTest, CompilesAndPlaysEveryReleasedPreset) {
    const auto manifest =
        juce::JSON::parse(presetRoot().getChildFile("manifest.json").loadFileAsString());
    ASSERT_TRUE(manifest.isObject());
    EXPECT_EQ(manifest.getProperty("sourceRelease", {}).toString(),
              RIFFRA_SONALLOY_TEST_SOURCE_RELEASE);
    const auto manifestPresets = manifest.getProperty("presets", {});
    ASSERT_TRUE(manifestPresets.isArray());

    const auto directories = presetRoot().findChildFiles(juce::File::findDirectories, false, "*");
    ASSERT_FALSE(directories.isEmpty());
    std::vector<juce::String> stagedPresetIds;
    for (const auto& directory : directories) {
        const auto definition = directory.getChildFile("definition.json");
        if (!definition.existsAsFile()) continue;
        stagedPresetIds.push_back(directory.getFileName());
        juce::String error;
        auto runtime = loadPreset(directory, error);
        ASSERT_NE(runtime, nullptr)
            << directory.getFileName().toStdString() << ": " << error.toStdString();

        juce::AudioBuffer<float> output(2, 256);
        juce::MidiBuffer midi;
        midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
        runtime->process(output.getArrayOfWritePointers(), 2, 256, &midi, playingContext());
        expectFinite(output);
        ASSERT_GT(maximumMagnitude(output), 0.0f) << directory.getFileName().toStdString();
        ASSERT_EQ(runtime->faultCode(), 0u) << directory.getFileName().toStdString();
    }
    std::vector<juce::String> manifestPresetIds;
    for (const auto& preset : *manifestPresets.getArray()) {
        manifestPresetIds.push_back(preset.toString());
        ASSERT_TRUE(presetRoot()
                        .getChildFile(preset.toString())
                        .getChildFile("definition.json")
                        .existsAsFile())
            << preset.toString().toStdString();
    }
    const auto comparePresetIds = [](const juce::String& left, const juce::String& right) {
        return left.compare(right) < 0;
    };
    std::sort(stagedPresetIds.begin(), stagedPresetIds.end(), comparePresetIds);
    std::sort(manifestPresetIds.begin(), manifestPresetIds.end(), comparePresetIds);
    ASSERT_EQ(manifestPresetIds, stagedPresetIds);
}

TEST(SonalloyInstrumentRuntimeTest, NoteOnProducesFiniteNonZeroStereoOutput) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    processBlock(*runtime, output, &midi);

    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, NoteOffReleasesTheNewestVoice) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    processBlock(*runtime, output, &midi);
    const auto soundingMagnitude = maximumMagnitude(output);
    ASSERT_GT(soundingMagnitude, 0.0f);

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    processBlock(*runtime, output, &midi);
    for (int block = 0; block < 64; ++block) processBlock(*runtime, output);

    expectFinite(output);
    EXPECT_LT(maximumMagnitude(output), soundingMagnitude * 0.1f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, SustainPedalDefersAndThenReleasesNoteOff) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    processBlock(*runtime, output, &midi);

    midi.clear();
    midi.addEvent(juce::MidiMessage::controllerEvent(1, 64, 127), 0);
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 1);
    processBlock(*runtime, output, &midi);
    const auto heldMagnitude = maximumMagnitude(output);
    expectFinite(output);
    EXPECT_GT(heldMagnitude, 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);

    midi.clear();
    midi.addEvent(juce::MidiMessage::controllerEvent(1, 64, 0), 0);
    processBlock(*runtime, output, &midi);
    for (int block = 0; block < 64; ++block) processBlock(*runtime, output);

    expectFinite(output);
    EXPECT_LT(maximumMagnitude(output), heldMagnitude * 0.1f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, PitchBendEventsAreAccepted) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    midi.addEvent(juce::MidiMessage::pitchWheel(1, 12'288), 32);
    processBlock(*runtime, output, &midi);

    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, ModWheelEventsAreAccepted) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    midi.addEvent(juce::MidiMessage::controllerEvent(1, 1, 96), 32);
    processBlock(*runtime, output, &midi);

    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, ChannelPressureEventsAreAccepted) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    midi.addEvent(juce::MidiMessage::channelPressureChange(1, 72), 32);
    processBlock(*runtime, output, &midi);

    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, RepeatedNotesReleaseNewestVoiceFirst) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.7f), 1);
    runtime->process(output.getArrayOfWritePointers(), 2, 256, &midi, playingContext());

    output.clear();
    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    runtime->process(output.getArrayOfWritePointers(), 2, 256, &midi, playingContext());
    ASSERT_EQ(runtime->faultCode(), 0u);
    expectFinite(output);
}

TEST(SonalloyInstrumentRuntimeTest, SameOffsetNoteOffPrecedesNewNoteOn) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    processBlock(*runtime, output, &midi);
    const auto soundingMagnitude = maximumMagnitude(output);
    ASSERT_GT(soundingMagnitude, 0.0f);

    // The input order intentionally places Note On before Note Off. The
    // adapter must still release the voice that existed before this block,
    // because same-offset output order puts Note Off first.
    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.7f), 0);
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    processBlock(*runtime, output, &midi);

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOff(1, 60), 0);
    processBlock(*runtime, output, &midi);
    for (int block = 0; block < 64; ++block) processBlock(*runtime, output);

    expectFinite(output);
    EXPECT_LT(maximumMagnitude(output), soundingMagnitude * 0.1f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, ResetAndBypassRemainRealtimeSafe) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    runtime->process(output.getArrayOfWritePointers(), 2, 256, &midi, playingContext());
    runtime->setBypassed(true);
    output.clear();
    runtime->process(output.getArrayOfWritePointers(), 2, 256, nullptr, playingContext());
    EXPECT_FLOAT_EQ(output.getMagnitude(0, 0, output.getNumSamples()), 0.0f);
    EXPECT_FLOAT_EQ(output.getMagnitude(1, 0, output.getNumSamples()), 0.0f);
    runtime->resetForTransportDiscontinuity();
    runtime->setBypassed(false);
    output.clear();
    runtime->process(output.getArrayOfWritePointers(), 2, 256, nullptr, playingContext());
    expectFinite(output);
    EXPECT_FLOAT_EQ(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    processBlock(*runtime, output, &midi);
    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, ResetRestartsRuntimeLocalFrameAndPreservesContinuity) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    for (int block = 0; block < 4; ++block)
        processBlock(*runtime, output, block == 0 ? &midi : nullptr);

    runtime->resetForTransportDiscontinuity();
    processBlock(*runtime, output);
    expectFinite(output);
    EXPECT_FLOAT_EQ(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);

    midi.clear();
    midi.addEvent(juce::MidiMessage::noteOn(1, 64, 0.7f), 0);
    processBlock(*runtime, output, &midi);
    expectFinite(output);
    EXPECT_GT(maximumMagnitude(output), 0.0f);
    EXPECT_EQ(runtime->faultCode(), 0u);
}

TEST(SonalloyInstrumentRuntimeTest, ProcessContextRemainsContinuousAcrossBlocks) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    midi.addEvent(juce::MidiMessage::noteOn(1, 60, 0.8f), 0);
    auto context = playingContext();
    for (int block = 0; block < 8; ++block) {
        context.absoluteFrame = static_cast<std::uint64_t>(block * output.getNumSamples());
        context.beatPosition = static_cast<double>(block) * 0.5;
        context.barPosition = static_cast<double>(block) / 4.0;
        processBlock(*runtime, output, block == 0 ? &midi : nullptr, context);
        expectFinite(output);
        EXPECT_EQ(runtime->faultCode(), 0u);
    }
}

TEST(SonalloyInstrumentRuntimeTest, RejectsAudioInputDefinitionsBeforeActivation) {
    const auto definition = sourceRoot()
                                .getChildFile("review/external-audio-cross-synthesis/definitions/")
                                .getChildFile("envelope-transfer-rhythm.json");
    ASSERT_TRUE(definition.existsAsFile()) << definition.getFullPathName().toStdString();

    juce::String error;
    auto runtime = SonalloyInstrumentRuntime::create(
        definition.loadFileAsString(), definition.getParentDirectory().getFullPathName(), 48'000.0,
        256, error);
    EXPECT_EQ(runtime, nullptr);
    EXPECT_NE(error.indexOf("audio input route"), -1);
}

TEST(SonalloyInstrumentRuntimeTest, MalformedDefinitionsReturnReadableErrors) {
    juce::String error;
    auto runtime = SonalloyInstrumentRuntime::create(
        "{", presetRoot().getChildFile("01-clean-sub-bass").getFullPathName(), 48'000.0, 256,
        error);
    EXPECT_EQ(runtime, nullptr);
    EXPECT_NE(error.indexOf("Built-in instrument definition compilation failed"), -1);
    EXPECT_FALSE(error.isEmpty());
}

TEST(SonalloyInstrumentRuntimeTest, EventCapacityOverflowFailsSafely) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    juce::MidiBuffer midi;
    for (int index = 0; index < 1'025; ++index)
        midi.addEvent(juce::MidiMessage::noteOn(1, 60 + (index % 12), 0.5f), 0);

    processBlock(*runtime, output, &midi);

    expectFinite(output);
    EXPECT_NE(runtime->faultCode(), 0u);
    EXPECT_FLOAT_EQ(maximumMagnitude(output), 0.0f);
}

TEST(SonalloyInstrumentRuntimeTest, ReportsQueuedMidiOverflowOnce) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    for (int index = 0; index < 257; ++index)
        (void)runtime->enqueueMidi(juce::MidiMessage::noteOn(1, 60, 0.5f));

    EXPECT_EQ(runtime->droppedMidiEvents(), 1u);
}

TEST(SonalloyInstrumentRuntimeTest, IgnoresMaximumSizedSysExWithoutCallbackAllocation) {
    juce::String error;
    auto runtime = loadPreset(presetRoot().getChildFile("01-clean-sub-bass"), error);
    ASSERT_NE(runtime, nullptr) << error.toStdString();

    juce::AudioBuffer<float> output(2, 256);
    processBlock(*runtime, output);

    std::array<std::uint8_t, 256> raw{};
    raw.front() = 0xf0;
    raw.back() = 0xf7;
    const juce::MidiMessage liveMessage(raw.data(), static_cast<int>(raw.size()));
    ASSERT_TRUE(runtime->enqueueMidi(liveMessage));

    juce::MidiBuffer timelineMidi;
    ASSERT_TRUE(timelineMidi.addEvent(raw.data(), static_cast<int>(raw.size()), 0));

    ScopedAllocationCounter allocationCounter;
    processBlock(*runtime, output, &timelineMidi);

#if defined(RIFFRA_TRACK_MALLOC) || (defined(_MSC_VER) && defined(_DEBUG))
    EXPECT_EQ(allocationCounter.count(), 0u);
#else
    GTEST_SKIP() << "callback allocation tracking is unavailable on this platform";
#endif
    EXPECT_EQ(runtime->faultCode(), 0u);
}

}  // namespace riffra
