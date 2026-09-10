#include "CommandRouting.h"

#include <array>

namespace riffra {
namespace {

template <std::size_t Size>
bool contains(const std::array<std::string_view, Size>& values,
              const std::string_view value) noexcept {
    for (const auto candidate : values)
        if (candidate == value) return true;
    return false;
}

constexpr std::array<std::string_view, 1> kShutdownCommands{"shutdown"};
constexpr std::array<std::string_view, 4> kSafetyCommands{
    "setEmergencyMute", "setFeedbackProtection", "setEngineTransitionMute", "setMasterGainDb"};
constexpr std::array<std::string_view, 4> kTimelineCommands{
    "loadTimelineSnapshot", "prepareTimelineSnapshot", "commitTimelineSnapshot",
    "discardTimelineSnapshot"};
constexpr std::array<std::string_view, 9> kTrackDeviceCommands{
    "setTrackDeviceBypassed",   "setTrackDeviceParameter", "getTrackDeviceStatus",
    "getTrackDeviceParameters", "getTrackDevicePrograms",  "getTrackPluginState",
    "setTrackPluginState",      "setTrackDeviceProgram",   "openTrackPluginEditor"};
constexpr std::array<std::string_view, 4> kTransportCommands{"playTimeline", "setTransportStarting",
                                                             "stopTimeline", "seekTimeline"};
constexpr std::array<std::string_view, 5> kMidiCommands{"enableMidiListening",
                                                        "disableMidiListening", "setLiveMidiTarget",
                                                        "sendTrackMidi", "panicTrackMidi"};
constexpr std::array<std::string_view, 5> kPreviewCommands{
    "startTakeComparison", "switchTakeComparisonVariant", "stopTakeComparison", "previewSample",
    "stopPreview"};
constexpr std::array<std::string_view, 2> kDeviceCommands{"recoverAudioDevice", "setAudioDriver"};
constexpr std::array<std::string_view, 2> kRecordingCommands{"startArrangeRecording",
                                                             "stopArrangeRecording"};
constexpr std::array<std::string_view, 2> kStatusCommands{"status", "meterStatus"};

}  // namespace

CommandFamily commandFamilyFor(const std::string_view type) noexcept {
    if (contains(kShutdownCommands, type)) return CommandFamily::shutdown;
    if (contains(kSafetyCommands, type)) return CommandFamily::safety;
    if (contains(kTimelineCommands, type)) return CommandFamily::timeline;
    if (contains(kTrackDeviceCommands, type)) return CommandFamily::trackDevice;
    if (contains(kTransportCommands, type)) return CommandFamily::transport;
    if (contains(kMidiCommands, type)) return CommandFamily::midi;
    if (contains(kPreviewCommands, type)) return CommandFamily::preview;
    if (contains(kDeviceCommands, type)) return CommandFamily::device;
    if (contains(kRecordingCommands, type)) return CommandFamily::recording;
    if (contains(kStatusCommands, type)) return CommandFamily::status;
    return CommandFamily::unsupported;
}

}  // namespace riffra
