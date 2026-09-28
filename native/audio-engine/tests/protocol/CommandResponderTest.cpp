#include <gtest/gtest.h>

#include <memory>
#include <vector>

#include "protocol/CommandResponder.h"

namespace riffra {
namespace {

CommandResponder::EnvelopeWriter recordInto(std::vector<juce::var>& written) {
    return [&written](const juce::var& envelope) { written.push_back(envelope); };
}

TEST(CommandResponderTest, ReportsNoResponseWhenDestroyedUnanswered) {
    std::vector<juce::var> written;

    auto responder = std::make_unique<CommandResponder>(7, recordInto(written));
    responder.reset();

    ASSERT_EQ(written.size(), 1u);
    EXPECT_EQ(written[0].getProperty("kind", {}).toString(), "error");
    EXPECT_EQ(static_cast<juce::int64>(written[0].getProperty("requestId", {})), 7);
    EXPECT_EQ(written[0].getProperty("error", {}).getProperty("kind", {}).toString(), "noResponse");
}

TEST(CommandResponderTest, WritesOnlyTheFirstCompletion) {
    std::vector<juce::var> written;
    {
        CommandResponder responder(8, recordInto(written));

        responder.respond(AckSpec{});
        responder.fail("late", "A second completion must be ignored.", "test");
    }

    ASSERT_EQ(written.size(), 1u);
    EXPECT_EQ(written[0].getProperty("kind", {}).toString(), "response");
}

TEST(CommandResponderTest, MovedFromResponderDoesNotReport) {
    std::vector<juce::var> written;
    {
        CommandResponder original(9, recordInto(written));
        CommandResponder moved(std::move(original));

        moved.respond(AckSpec{});
    }

    EXPECT_EQ(written.size(), 1u);
}

}  // namespace
}  // namespace riffra
