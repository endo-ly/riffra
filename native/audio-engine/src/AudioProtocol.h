#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <string>

namespace riffra {

enum class OutputKind { control, state, telemetry };

void clearCurrentRequestId() noexcept;
void setCurrentRequestId(const juce::String& requestId);
[[nodiscard]] juce::String currentRequestId();

[[nodiscard]] std::uint64_t droppedTelemetryCount() noexcept;
[[nodiscard]] std::uint64_t droppedStateCount() noexcept;

[[nodiscard]] juce::var makeError(const juce::String& scope, const juce::String& message);
bool parseMidiBytes(const juce::var& value, juce::MidiMessage& message, juce::String& error);
void writeJson(const juce::var& value, const juce::String& requestId = {},
               OutputKind kind = OutputKind::control, std::string stateKey = {});

}  // namespace riffra
