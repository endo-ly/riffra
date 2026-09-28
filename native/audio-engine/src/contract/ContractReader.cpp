#include "ContractReader.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

namespace riffra {

ContractReader::ContractReader(const juce::var& value, juce::String objectPath,
                               const std::initializer_list<const char*> expected,
                               juce::String& errorMessage)
    : source(value), path(std::move(objectPath)), error(errorMessage) {
    expectedKeys.reserve(expected.size());
    for (const auto* key : expected) expectedKeys.emplace_back(key);
    if (!source.isObject() && error.isEmpty()) error = path + ": expected object";
}

juce::String ContractReader::fieldPath(const char* key) const {
    return path.isEmpty() ? juce::String(key) : path + "." + key;
}

bool ContractReader::fail(const char* key, const juce::String& message) {
    if (error.isEmpty()) error = fieldPath(key) + ": " + message;
    return false;
}

bool ContractReader::read(const char* key, juce::var& output) {
    if (!error.isEmpty()) return false;
    const juce::Identifier identifier(key);
    if (std::find(expectedKeys.begin(), expectedKeys.end(), identifier) == expectedKeys.end())
        return fail(key, "reader did not declare this key");
    const auto* object = source.getDynamicObject();
    if (object == nullptr || !object->hasProperty(identifier))
        return fail(key, "missing required key");
    output = object->getProperty(identifier);
    return true;
}

bool ContractReader::value(const char* key, juce::var& output) { return read(key, output); }

bool ContractReader::object(const char* key, juce::var& output) {
    if (!read(key, output)) return false;
    return output.isObject() || fail(key, "expected object");
}

bool ContractReader::array(const char* key, juce::Array<juce::var>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (!item.isArray()) return fail(key, "expected array");
    output = *item.getArray();
    return true;
}

bool ContractReader::string(const char* key, juce::String& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (!item.isString()) return fail(key, "expected string");
    output = item.toString();
    return true;
}

bool ContractReader::optionalString(const char* key, std::optional<juce::String>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (item.isVoid()) {
        output.reset();
        return true;
    }
    if (!item.isString()) return fail(key, "expected string or null");
    output = item.toString();
    return true;
}

bool ContractReader::boolean(const char* key, bool& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (!item.isBool()) return fail(key, "expected boolean");
    output = static_cast<bool>(item);
    return true;
}

bool ContractReader::number(const char* key, double& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (!item.isDouble() && !item.isInt() && !item.isInt64()) return fail(key, "expected number");
    output = static_cast<double>(item);
    if (!std::isfinite(output)) return fail(key, "number must be finite");
    return true;
}

bool ContractReader::optionalNumber(const char* key, std::optional<double>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (item.isVoid()) {
        output.reset();
        return true;
    }
    double value = 0.0;
    if (!number(key, value)) return false;
    output = value;
    return true;
}

bool ContractReader::unsignedInteger(const char* key, std::uint64_t& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (!item.isInt() && !item.isInt64()) return fail(key, "expected integer");
    const auto numberValue = static_cast<juce::int64>(item);
    if (numberValue < 0) return fail(key, "integer must be non-negative");
    output = static_cast<std::uint64_t>(numberValue);
    return true;
}

bool ContractReader::optionalUnsignedInteger(const char* key,
                                             std::optional<std::uint64_t>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (item.isVoid()) {
        output.reset();
        return true;
    }
    std::uint64_t value = 0;
    if (!unsignedInteger(key, value)) return false;
    output = value;
    return true;
}

bool ContractReader::unsigned32(const char* key, std::uint32_t& output) {
    std::uint64_t value = 0;
    if (!unsignedInteger(key, value)) return false;
    if (value > std::numeric_limits<std::uint32_t>::max())
        return fail(key, "integer exceeds u32 range");
    output = static_cast<std::uint32_t>(value);
    return true;
}

bool ContractReader::optionalUnsigned32(const char* key, std::optional<std::uint32_t>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (item.isVoid()) {
        output.reset();
        return true;
    }
    std::uint32_t value = 0;
    if (!unsigned32(key, value)) return false;
    output = value;
    return true;
}

bool ContractReader::unsigned8(const char* key, std::uint8_t& output) {
    std::uint64_t value = 0;
    if (!unsignedInteger(key, value)) return false;
    if (value > std::numeric_limits<std::uint8_t>::max())
        return fail(key, "integer exceeds u8 range");
    output = static_cast<std::uint8_t>(value);
    return true;
}

bool ContractReader::optionalUnsigned8(const char* key, std::optional<std::uint8_t>& output) {
    juce::var item;
    if (!read(key, item)) return false;
    if (item.isVoid()) {
        output.reset();
        return true;
    }
    if (!item.isInt() && !item.isInt64()) return fail(key, "expected integer or null");
    const auto numberValue = static_cast<juce::int64>(item);
    if (numberValue < 0 || numberValue > std::numeric_limits<std::uint8_t>::max())
        return fail(key, "integer is outside u8 range");
    output = static_cast<std::uint8_t>(numberValue);
    return true;
}

bool ContractReader::finish() {
    if (!error.isEmpty()) return false;
    const auto* object = source.getDynamicObject();
    if (object == nullptr) return false;
    const auto& properties = object->getProperties();
    for (int index = 0; index < properties.size(); ++index) {
        const auto name = properties.getName(index);
        if (std::find(expectedKeys.begin(), expectedKeys.end(), name) == expectedKeys.end()) {
            if (error.isEmpty())
                error = (path.isEmpty() ? name.toString() : path + "." + name.toString()) +
                        ": unknown key";
            return false;
        }
    }
    return true;
}

}  // namespace riffra
