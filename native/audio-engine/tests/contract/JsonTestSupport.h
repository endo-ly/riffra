#pragma once

#include <JuceHeader.h>

#include <functional>
#include <string>
#include <utility>
#include <vector>

namespace riffra::json_test {

using JsonPath = std::vector<std::string>;

inline juce::var cloneJson(const juce::var& value) {
    if (value.isArray()) {
        juce::Array<juce::var> clone;
        for (const auto& item : *value.getArray()) clone.add(cloneJson(item));
        return clone;
    }
    if (value.isObject()) {
        auto* clone = new juce::DynamicObject();
        const auto& properties = value.getDynamicObject()->getProperties();
        for (int index = 0; index < properties.size(); ++index)
            clone->setProperty(properties.getName(index), cloneJson(properties.getValueAt(index)));
        return juce::var(clone);
    }
    return value;
}

inline juce::var readJsonFile(const juce::File& file) {
    return juce::JSON::parse(file.loadFileAsString());
}

/// Collects the path of every object, including the root.
inline void collectObjectPaths(const juce::var& value, JsonPath& path,
                               std::vector<JsonPath>& output) {
    if (value.isArray()) {
        const auto& items = *value.getArray();
        for (int index = 0; index < items.size(); ++index) {
            path.push_back("#" + std::to_string(index));
            collectObjectPaths(items.getReference(index), path, output);
            path.pop_back();
        }
    } else if (value.isObject()) {
        output.push_back(path);
        const auto& properties = value.getDynamicObject()->getProperties();
        for (int index = 0; index < properties.size(); ++index) {
            path.push_back(properties.getName(index).toString().toStdString());
            collectObjectPaths(properties.getValueAt(index), path, output);
            path.pop_back();
        }
    }
}

/// Collects every object key as (path of its object, key).
inline void collectFieldPaths(const juce::var& value, JsonPath& path,
                              std::vector<std::pair<JsonPath, std::string>>& output) {
    if (value.isArray()) {
        const auto& items = *value.getArray();
        for (int index = 0; index < items.size(); ++index) {
            path.push_back("#" + std::to_string(index));
            collectFieldPaths(items.getReference(index), path, output);
            path.pop_back();
        }
    } else if (value.isObject()) {
        const auto& properties = value.getDynamicObject()->getProperties();
        for (int index = 0; index < properties.size(); ++index) {
            const auto key = properties.getName(index).toString().toStdString();
            output.emplace_back(path, key);
            path.push_back(key);
            collectFieldPaths(properties.getValueAt(index), path, output);
            path.pop_back();
        }
    }
}

inline juce::String formatPath(const JsonPath& path) {
    juce::String result;
    for (const auto& token : path) {
        if (!token.empty() && token.front() == '#') {
            const auto index = token.substr(1);
            result += "[" + juce::String::fromUTF8(index.c_str()) + "]";
        } else {
            result += (result.isEmpty() ? "" : ".") + juce::String::fromUTF8(token.c_str());
        }
    }
    return result;
}

inline juce::var mutateAtPath(const juce::var& root, const JsonPath& path, const std::size_t index,
                              const std::function<void(juce::var&)>& mutate) {
    if (index == path.size()) {
        auto result = cloneJson(root);
        mutate(result);
        return result;
    }
    const auto& token = path[index];
    if (!token.empty() && token.front() == '#') {
        const auto itemIndex = std::stoi(token.substr(1));
        auto result = cloneJson(root);
        auto items = *result.getArray();
        items.set(itemIndex, mutateAtPath(items.getReference(itemIndex), path, index + 1, mutate));
        return items;
    }
    auto result = cloneJson(root);
    auto* object = result.getDynamicObject();
    const auto identifier = juce::Identifier(juce::String::fromUTF8(token.c_str()));
    const auto child = root.getDynamicObject()->getProperty(identifier);
    object->setProperty(identifier, mutateAtPath(child, path, index + 1, mutate));
    return result;
}

inline juce::var withoutKey(const juce::var& root, const JsonPath& objectPath,
                            const std::string& key) {
    return mutateAtPath(root, objectPath, 0, [&key](juce::var& target) {
        target.getDynamicObject()->removeProperty(
            juce::Identifier(juce::String::fromUTF8(key.c_str())));
    });
}

inline juce::var withUnknownKey(const juce::var& root, const JsonPath& objectPath) {
    return mutateAtPath(root, objectPath, 0, [](juce::var& target) {
        target.getDynamicObject()->setProperty("__unexpected", false);
    });
}

inline void setValue(juce::var& root, const JsonPath& path, const juce::var& value) {
    root = mutateAtPath(root, path, 0, [&value](juce::var& target) { target = value; });
}

/// Compares JSON trees by value, ignoring object key order.
inline bool jsonEquals(const juce::var& left, const juce::var& right) {
    if (left.isArray() || right.isArray()) {
        if (!left.isArray() || !right.isArray() || left.size() != right.size()) return false;
        for (int index = 0; index < left.size(); ++index)
            if (!jsonEquals(left[index], right[index])) return false;
        return true;
    }
    if (left.isObject() || right.isObject()) {
        if (!left.isObject() || !right.isObject()) return false;
        const auto& leftProperties = left.getDynamicObject()->getProperties();
        const auto& rightProperties = right.getDynamicObject()->getProperties();
        if (leftProperties.size() != rightProperties.size()) return false;
        for (int index = 0; index < leftProperties.size(); ++index) {
            const auto name = leftProperties.getName(index);
            if (!rightProperties.contains(name) ||
                !jsonEquals(leftProperties.getValueAt(index), rightProperties[name]))
                return false;
        }
        return true;
    }
    const auto integral = [](const juce::var& value) { return value.isInt() || value.isInt64(); };
    if (integral(left) || integral(right))
        return integral(left) && integral(right) &&
               static_cast<juce::int64>(left) == static_cast<juce::int64>(right);
    if (left.isDouble() || right.isDouble())
        return left.isDouble() && right.isDouble() &&
               static_cast<double>(left) == static_cast<double>(right);
    if (left.isString() || right.isString())
        return left.isString() && right.isString() && left.toString() == right.toString();
    if (left.isBool() || right.isBool())
        return left.isBool() && right.isBool() &&
               static_cast<bool>(left) == static_cast<bool>(right);
    return left.isVoid() && right.isVoid();
}

}  // namespace riffra::json_test
