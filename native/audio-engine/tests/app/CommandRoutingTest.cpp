#include <gtest/gtest.h>

#include <initializer_list>
#include <string_view>

#include "app/CommandRouting.h"

namespace riffra {
namespace {

TEST(CommandRoutingTest, PreservesCommandFamilyAssignments) {
    const auto expectFamily = [](const CommandFamily family,
                                 const std::initializer_list<std::string_view> commands) {
        for (const auto command : commands) EXPECT_EQ(commandFamilyFor(command), family) << command;
    };

    expectFamily(CommandFamily::shutdown, {"shutdown"});
    expectFamily(CommandFamily::safety, {"setEmergencyMute", "setFeedbackProtection",
                                         "setEngineTransitionMute", "setMasterGainDb"});
    expectFamily(CommandFamily::timeline, {"loadTimelineSnapshot", "prepareTimelineSnapshot",
                                           "commitTimelineSnapshot", "discardTimelineSnapshot"});
    expectFamily(CommandFamily::trackDevice,
                 {"setTrackDeviceBypassed", "setTrackDeviceParameter", "getTrackDeviceStatus",
                  "getTrackDeviceParameters", "getTrackDevicePrograms", "getTrackPluginState",
                  "setTrackPluginState", "setTrackDeviceProgram", "openTrackPluginEditor"});
    expectFamily(CommandFamily::transport,
                 {"playTimeline", "setTransportStarting", "stopTimeline", "seekTimeline"});
    expectFamily(CommandFamily::midi, {"enableMidiListening", "disableMidiListening",
                                       "setLiveMidiTarget", "sendTrackMidi", "panicTrackMidi"});
    expectFamily(CommandFamily::preview, {"startTakeComparison", "switchTakeComparisonVariant",
                                          "stopTakeComparison", "previewSample", "stopPreview"});
    expectFamily(CommandFamily::device, {"recoverAudioDevice", "setAudioDriver"});
    expectFamily(CommandFamily::recording, {"startArrangeRecording", "stopArrangeRecording"});
    expectFamily(CommandFamily::status, {"status", "meterStatus"});

    EXPECT_EQ(commandFamilyFor("unknown"), CommandFamily::unsupported);
}

}  // namespace
}  // namespace riffra
