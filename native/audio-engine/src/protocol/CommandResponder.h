#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <functional>

#include "contract/SidecarMessages.h"

namespace riffra {

/// Completes one sidecar request with exactly one response or error.
///
/// The responder is move-only. A second completion is ignored, and a responder
/// destroyed without completing its request reports a `noResponse` error.
class CommandResponder final {
public:
    using EnvelopeWriter = std::function<void(const juce::var&)>;

    explicit CommandResponder(std::uint64_t requestId, EnvelopeWriter writer);
    ~CommandResponder();

    CommandResponder(CommandResponder&& other) noexcept;
    CommandResponder& operator=(CommandResponder&&) = delete;
    CommandResponder(const CommandResponder&) = delete;
    CommandResponder& operator=(const CommandResponder&) = delete;

    void respond(const SidecarResponseSpec& response);
    void fail(const juce::String& kind, const juce::String& message, const juce::String& operation,
              const juce::var& details = {});

private:
    [[nodiscard]] bool claim() noexcept;

    std::uint64_t requestId;
    EnvelopeWriter writer;
    bool replied = false;
};

}  // namespace riffra
