#include <optional>

#include "../AudioCommandDispatcher.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

std::optional<float> toFloat(const std::optional<double>& value) {
    return value.has_value() ? std::optional<float>(static_cast<float>(*value)) : std::nullopt;
}

}  // namespace

void AudioCommandDispatcher::handle(const SetChannelMixCommand& command,
                                    CommandResponder responder) {
    juce::String error;
    if (!context.timelineEngine.setChannelMixControl(command.owner, toFloat(command.gainDb),
                                                     toFloat(command.pan), error)) {
        responder.fail("trackMix", error, "mixer.channel.preview");
        return;
    }
    responder.respond(MixAckSpec{});
}

void AudioCommandDispatcher::handle(const SetSendLevelCommand& command,
                                    CommandResponder responder) {
    juce::String error;
    if (!context.timelineEngine.setSendLevelControl(command.sendId,
                                                    static_cast<float>(command.levelDb), error)) {
        responder.fail("sendMix", error, "mixer.send.preview");
        return;
    }
    responder.respond(MixAckSpec{});
}

}  // namespace riffra
