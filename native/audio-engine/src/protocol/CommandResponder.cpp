#include "CommandResponder.h"

#include <utility>

namespace riffra {

CommandResponder::CommandResponder(const std::uint64_t id, EnvelopeWriter envelopeWriter)
    : requestId(id), writer(std::move(envelopeWriter)) {}

CommandResponder::CommandResponder(CommandResponder&& other) noexcept
    : requestId(other.requestId),
      writer(std::move(other.writer)),
      replied(std::exchange(other.replied, true)) {}

CommandResponder::~CommandResponder() {
    if (!replied)
        fail("noResponse", "The native command finished without a response.", "sidecar.respond");
}

bool CommandResponder::claim() noexcept {
    if (replied) {
        jassertfalse;
        return false;
    }
    replied = true;
    return true;
}

void CommandResponder::respond(const SidecarResponseSpec& response) {
    if (claim()) writer(encodeResponse(requestId, response));
}

void CommandResponder::fail(const juce::String& kind, const juce::String& message,
                            const juce::String& operation, const juce::var& details) {
    if (claim()) writer(encodeError(requestId, {kind, message, operation, details}));
}

}  // namespace riffra
