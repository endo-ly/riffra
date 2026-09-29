#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <optional>
#include <variant>
#include <vector>

namespace riffra {

enum class TrackKindSpec { audio, instrument };
enum class FadeShapeSpec { linear, equalPower, smooth };
enum class TakeVariantSpec { raw, processed };
enum class MidiEventKindSpec { controlChange, pitchBend, channelPressure };

struct TimebaseSpec final {
    std::uint32_t ppq = 0;
    double bpm = 0.0;
    std::uint8_t timeSignatureNumerator = 0;
    std::uint8_t timeSignatureDenominator = 0;
    bool operator==(const TimebaseSpec&) const = default;
};

struct TickRangeSpec final {
    std::uint64_t startTick = 0;
    std::uint64_t endTick = 0;
    bool operator==(const TickRangeSpec&) const = default;
};

struct LoopRangeSpec final {
    bool enabled = false;
    std::uint64_t startTick = 0;
    std::uint64_t endTick = 0;
    bool operator==(const LoopRangeSpec&) const = default;
};

struct AudioInputSpec final {
    std::uint32_t channelIndex = 0;
    bool operator==(const AudioInputSpec&) const = default;
};

struct MidiInputSpec final {
    std::optional<juce::String> deviceId;
    std::optional<std::uint8_t> channel;
    bool operator==(const MidiInputSpec&) const = default;
};

struct AutomationPointSpec final {
    std::uint64_t tick = 0;
    double value = 0.0;
    bool operator==(const AutomationPointSpec&) const = default;
};

struct PluginStateSpec final {
    std::optional<juce::String> stateData;
    std::vector<float> parameterValues;
    bool bypassed = false;
    bool operator==(const PluginStateSpec&) const = default;
};

struct PluginDeviceSpec final {
    juce::String id;
    juce::String path;
    PluginStateSpec state;
    bool operator==(const PluginDeviceSpec&) const = default;
};

struct Vst3InstrumentSpec final {
    juce::String id;
    juce::String path;
    PluginStateSpec state;
    bool operator==(const Vst3InstrumentSpec&) const = default;
};

struct InternalInstrumentSpec final {
    juce::String id;
    bool bypassed = false;
    juce::String definitionJson;
    juce::String definitionBaseDir;
    bool operator==(const InternalInstrumentSpec&) const = default;
};

using InstrumentSpec = std::variant<Vst3InstrumentSpec, InternalInstrumentSpec>;

inline bool sameEffectTopology(const std::vector<PluginDeviceSpec>& left,
                               const std::vector<PluginDeviceSpec>& right) noexcept {
    if (left.size() != right.size()) return false;
    for (std::size_t index = 0; index < left.size(); ++index) {
        if (left[index].id != right[index].id || left[index].path != right[index].path)
            return false;
    }
    return true;
}

inline bool sameInstrumentTopology(const std::optional<InstrumentSpec>& left,
                                   const std::optional<InstrumentSpec>& right) noexcept {
    if (left.has_value() != right.has_value()) return false;
    if (!left.has_value()) return true;
    if (left->index() != right->index()) return false;
    if (const auto* leftVst3 = std::get_if<Vst3InstrumentSpec>(&*left)) {
        const auto& rightVst3 = std::get<Vst3InstrumentSpec>(*right);
        return leftVst3->id == rightVst3.id && leftVst3->path == rightVst3.path;
    }
    const auto& leftInternal = std::get<InternalInstrumentSpec>(*left);
    const auto& rightInternal = std::get<InternalInstrumentSpec>(*right);
    return leftInternal.id == rightInternal.id &&
           leftInternal.definitionJson == rightInternal.definitionJson &&
           leftInternal.definitionBaseDir == rightInternal.definitionBaseDir;
}

struct AudioClipSpec final {
    juce::String id;
    juce::String path;
    std::uint32_t sourceSampleRate = 0;
    std::uint64_t sourceStartFrame = 0;
    std::uint64_t sourceEndFrame = 0;
    std::uint64_t durationFrames = 0;
    std::uint32_t durationSampleRate = 0;
    std::uint64_t startTick = 0;
    std::uint64_t fadeInFrames = 0;
    std::uint64_t fadeOutFrames = 0;
    FadeShapeSpec fadeShape = FadeShapeSpec::linear;
    double gainDb = 0.0;
    double pan = 0.0;
    TakeVariantSpec takeVariant = TakeVariantSpec::raw;
    bool loopEnabled = false;
    bool muted = false;
    bool operator==(const AudioClipSpec&) const = default;
};

struct MidiNoteSpec final {
    std::uint64_t startTick = 0;
    std::uint64_t durationTicks = 0;
    std::uint8_t note = 0;
    std::uint8_t velocity = 0;
    std::uint8_t channel = 0;
    bool operator==(const MidiNoteSpec&) const = default;
};

struct MidiEventSpec final {
    MidiEventKindSpec kind = MidiEventKindSpec::controlChange;
    std::uint64_t tick = 0;
    std::uint8_t channel = 0;
    std::uint8_t data1 = 0;
    std::uint8_t data2 = 0;
    bool operator==(const MidiEventSpec&) const = default;
};

struct MidiClipSpec final {
    juce::String id;
    std::uint64_t startTick = 0;
    std::uint64_t durationTicks = 0;
    bool loopEnabled = false;
    bool muted = false;
    std::vector<MidiNoteSpec> notes;
    std::vector<MidiEventSpec> events;
    bool operator==(const MidiClipSpec&) const = default;
};

struct TrackSpec final {
    juce::String id;
    TrackKindSpec kind = TrackKindSpec::audio;
    double gainDb = 0.0;
    double pan = 0.0;
    bool muted = false;
    bool solo = false;
    bool armed = false;
    bool monitorInput = false;
    std::optional<AudioInputSpec> audioInput;
    MidiInputSpec midiInput;
    std::vector<AutomationPointSpec> volumeAutomation;
    std::vector<AutomationPointSpec> panAutomation;
    std::vector<PluginDeviceSpec> effects;
    std::optional<InstrumentSpec> instrument;
    std::vector<AudioClipSpec> audioClips;
    std::vector<MidiClipSpec> midiClips;
    bool operator==(const TrackSpec&) const = default;
};

struct ExecutionGraph final {
    TimebaseSpec timebase;
    LoopRangeSpec loopRange;
    std::optional<TickRangeSpec> punchRange;
    bool metronomeEnabled = false;
    double masterGainDb = 0.0;
    std::vector<TrackSpec> tracks;
    bool operator==(const ExecutionGraph&) const = default;
};

struct TimelineSnapshotSpec final {
    juce::String projectId;
    std::uint64_t revision = 0;
    ExecutionGraph graph;
    bool operator==(const TimelineSnapshotSpec&) const = default;
};

struct OfflineRenderRequestSpec final {
    ExecutionGraph graph;
    juce::String destination;
    std::uint64_t startTick = 0;
    std::uint64_t endTick = 0;
    std::uint32_t sampleRate = 0;
    std::uint32_t blockSize = 0;
    bool normalize = false;
    bool operator==(const OfflineRenderRequestSpec&) const = default;
};

}  // namespace riffra
