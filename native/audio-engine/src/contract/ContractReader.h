#pragma once

#include <JuceHeader.h>

#include <cstdint>
#include <initializer_list>
#include <optional>
#include <vector>

namespace riffra {

/// Reads one JSON object while enforcing its complete declared key set.
class ContractReader final {
public:
    ContractReader(const juce::var& value, juce::String path,
                   std::initializer_list<const char*> expectedKeys, juce::String& error);

    [[nodiscard]] bool value(const char* key, juce::var& output);
    [[nodiscard]] bool object(const char* key, juce::var& output);
    [[nodiscard]] bool array(const char* key, juce::Array<juce::var>& output);
    [[nodiscard]] bool string(const char* key, juce::String& output);
    [[nodiscard]] bool optionalString(const char* key, std::optional<juce::String>& output);
    [[nodiscard]] bool boolean(const char* key, bool& output);
    [[nodiscard]] bool number(const char* key, double& output);
    [[nodiscard]] bool unsignedInteger(const char* key, std::uint64_t& output);
    [[nodiscard]] bool unsigned32(const char* key, std::uint32_t& output);
    [[nodiscard]] bool unsigned8(const char* key, std::uint8_t& output);
    [[nodiscard]] bool optionalUnsigned8(const char* key, std::optional<std::uint8_t>& output);
    [[nodiscard]] bool finish();

private:
    [[nodiscard]] bool read(const char* key, juce::var& output);
    [[nodiscard]] bool fail(const char* key, const juce::String& message);
    [[nodiscard]] juce::String fieldPath(const char* key) const;

    const juce::var& source;
    juce::String path;
    std::vector<juce::Identifier> expectedKeys;
    juce::String& error;
};

}  // namespace riffra
