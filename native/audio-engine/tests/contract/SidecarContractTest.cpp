#include <gtest/gtest.h>

#include <string>
#include <utility>
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

TEST(SidecarContractTest, RejectsMissingAndUnknownKeysAtEveryObjectDepth) {
    for (const auto type : kSidecarCommandTypes) {
        const auto fixture = commandFixture(juce::String(std::string(type).c_str()));
        JsonPath path;
        std::vector<std::pair<JsonPath, std::string>> fields;
        collectFieldPaths(fixture, path, fields);
        for (const auto& [objectPath, key] : fields) {
            juce::String error;
            auto fieldPath = objectPath;
            fieldPath.push_back(key);
            EXPECT_FALSE(decodes(withoutKey(fixture, objectPath, key), error))
                << type << ": removing " << formatPath(fieldPath) << " was accepted";
        }
        std::vector<JsonPath> objects;
        collectObjectPaths(fixture, path, objects);
        for (const auto& objectPath : objects) {
            juce::String error;
            EXPECT_FALSE(decodes(withUnknownKey(fixture, objectPath), error))
                << type << ": an unknown key at " << formatPath(objectPath) << " was accepted";
        }
    }
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
