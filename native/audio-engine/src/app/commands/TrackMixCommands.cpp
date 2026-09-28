#include <optional>

#include "../AudioCommandDispatcher.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

std::optional<float> toFloat(const std::optional<double>& value) {
    return value.has_value() ? std::optional<float>(static_cast<float>(*value)) : std::nullopt;
}

}  // namespace

void AudioCommandDispatcher::handle(const SetTrackMixCommand& command, CommandResponder responder) {
    juce::String error;
    if (!context.timelineEngine.setTrackMixControl(command.trackId, toFloat(command.gainDb),
                                                   toFloat(command.pan), error)) {
        responder.fail("trackMix", error, "track.mix.preview");
        return;
    }
    responder.respond(AckSpec{});
}

}  // namespace riffra
