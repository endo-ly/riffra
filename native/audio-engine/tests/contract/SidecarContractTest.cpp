#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "JsonTestSupport.h"
#include "contract/SidecarCommands.h"

namespace riffra {
namespace {

using namespace json_test;

juce::File commandFixtureDirectory() {
    return juce::File(RIFFRA_SIDECAR_FIXTURE_DIR).getChildFile("commands");
}

juce::var commandFixture(const juce::String& type) {
    return readJsonFile(commandFixtureDirectory().getChildFile(type + ".json"));
}

bool decodes(const juce::var& value, juce::String& error) {
    SidecarRequestSpec request;
    return decodeSidecarRequest(value, request, error);
}

TEST(SidecarContractTest, EveryAcceptedCommandHasARustFixture) {
    for (const auto type : kSidecarCommandTypes) {
        const auto name = juce::String(std::string(type).c_str());
        EXPECT_TRUE(commandFixtureDirectory().getChildFile(name + ".json").existsAsFile())
            << "missing command fixture: " << name;
    }
}

TEST(SidecarContractTest, DecodesEveryCommandFixture) {
    const auto files =
        commandFixtureDirectory().findChildFiles(juce::File::findFiles, false, "*.json");
    ASSERT_FALSE(files.isEmpty());
    for (const auto& file : files) {
        juce::String error;
        EXPECT_TRUE(decodes(readJsonFile(file), error)) << file.getFileName() << ": " << error;
    }
}

TEST(SidecarContractTest, DecodesEditorClosureOnTimelineIdle) {
    auto fixture = commandFixture("waitForTimelineIdle");
    setValue(fixture, {"command", "closeEditor"}, true);
    SidecarRequestSpec request;
    juce::String error;
    ASSERT_TRUE(decodeSidecarRequest(fixture, request, error)) << error;
    const auto* command = std::get_if<WaitForTimelineIdleCommand>(&request.command);
    ASSERT_NE(command, nullptr);
    EXPECT_TRUE(command->closeEditor);
}

TEST(SidecarContractTest, RejectsRepresentativeMalformedCommands) {
    const auto expectRejected = [](const char* mutation, const juce::var& value) {
        juce::String error;
        EXPECT_FALSE(decodes(value, error)) << mutation;
        EXPECT_TRUE(error.isNotEmpty()) << mutation;
    };

    // A numeric command covers required root and command fields, unknown keys,
    // and an incorrect field type.
    const auto seek = commandFixture("seekTimeline");
    expectRejected("missing requestId", withoutKey(seek, {}, "requestId"));
    expectRejected("unknown root field", withUnknownKey(seek, {}));
    expectRejected("missing seek tick", withoutKey(seek, {"command"}, "tick"));
    expectRejected("unknown seek field", withUnknownKey(seek, {"command"}));
    auto invalidTick = cloneJson(seek);
    setValue(invalidTick, {"command", "tick"}, "later");
    expectRejected("invalid seek tick type", invalidTick);

    // MIDI bytes have their own required and typed payload.
    const auto midi = commandFixture("sendTrackMidi");
    expectRejected("missing MIDI bytes", withoutKey(midi, {"command"}, "bytes"));
    expectRejected("unknown MIDI command field", withUnknownKey(midi, {"command"}));
    auto invalidBytes = cloneJson(midi);
    setValue(invalidBytes, {"command", "bytes"}, "not-midi-bytes");
    expectRejected("invalid MIDI bytes type", invalidBytes);

    // Snapshot commands carry nested objects whose schema is also strict.
    const auto snapshot = commandFixture("prepareTimelineSnapshot");
    expectRejected("missing execution snapshot", withoutKey(snapshot, {"command"}, "snapshot"));
    expectRejected("unknown execution graph field",
                   withUnknownKey(snapshot, {"command", "snapshot", "graph"}));
    auto invalidSnapshot = cloneJson(snapshot);
    setValue(invalidSnapshot, {"command", "snapshot"}, "not-an-object");
    expectRejected("invalid nested snapshot type", invalidSnapshot);
}

TEST(SidecarContractTest, RejectsMalformedMidiBytes) {
    const auto fixture = commandFixture("sendTrackMidi");
    const std::vector<juce::Array<juce::var>> invalidBytes{
        {},
        {0x90, 60, 100, 0},
        {0x40, 60, 100},
        {0x90, 128, 100},
    };
    for (const auto& bytes : invalidBytes) {
        auto mutated = cloneJson(fixture);
        setValue(mutated, {"command", "bytes"}, bytes);
        juce::String error;
        EXPECT_FALSE(decodes(mutated, error));
    }
}

TEST(SidecarContractTest, ReadsTheRequestIdOfAnInvalidCommand) {
    auto invalid = commandFixture("seekTimeline");
    setValue(invalid, {"command", "tick"}, "later");

    juce::String error;
    EXPECT_FALSE(decodes(invalid, error));
    EXPECT_EQ(readSidecarRequestId(invalid), std::optional<std::uint64_t>(1));
}

}  // namespace
}  // namespace riffra
