#include <cmath>
#include <optional>

#include "../AudioCommandDispatcher.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

std::optional<float> optionalFloat(const juce::var& value, juce::String& error) {
    if (value.isVoid()) return std::nullopt;
    if (!value.isDouble() && !value.isInt() && !value.isInt64()) {
        error = "Track mix values must be numbers.";
        return std::nullopt;
    }
    const auto numeric = static_cast<double>(value);
    if (!std::isfinite(numeric)) {
        error = "Track mix values must be finite.";
        return std::nullopt;
    }
    const auto converted = static_cast<float>(numeric);
    if (!std::isfinite(converted)) {
        error = "Track mix values are outside the supported range.";
        return std::nullopt;
    }
    return converted;
}

}  // namespace

CommandResult AudioCommandDispatcher::dispatchTrackMix(const juce::var& command) {
    const auto trackId = command.getProperty("trackId", {}).toString();
    juce::String error;
    const auto gainDb = optionalFloat(command.getProperty("gainDb", {}), error);
    if (error.isNotEmpty()) {
        writeJson(makeError("invalidCommand", error, "track.mix.preview"));
        return {};
    }
    const auto pan = optionalFloat(command.getProperty("pan", {}), error);
    if (error.isNotEmpty()) {
        writeJson(makeError("invalidCommand", error, "track.mix.preview"));
        return {};
    }
    if (!context.timelineEngine.setTrackMixControl(trackId, gainDb, pan, error)) {
        writeJson(makeError("trackMix", error, "track.mix.preview"));
        return {};
    }

    auto* acknowledgement = new juce::DynamicObject();
    acknowledgement->setProperty("type", "trackMixAck");
    writeJson(juce::var(acknowledgement));
    return {};
}

}  // namespace riffra
