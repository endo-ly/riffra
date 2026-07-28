#pragma once

#include <JuceHeader.h>

namespace riffra::test {

class TemporaryDirectory final {
public:
    TemporaryDirectory() {
        directory = juce::File::getSpecialLocation(juce::File::tempDirectory)
            .getChildFile("riffra-audio-tests")
            .getChildFile(juce::Uuid().toString());
        directory.createDirectory();
    }

    ~TemporaryDirectory() { directory.deleteRecursively(); }

    TemporaryDirectory(const TemporaryDirectory&) = delete;
    TemporaryDirectory& operator=(const TemporaryDirectory&) = delete;

    [[nodiscard]] const juce::File& get() const noexcept { return directory; }

private:
    juce::File directory;
};

inline juce::var parseJsonFile(const juce::File& file) {
    return juce::JSON::parse(file.loadFileAsString());
}

} // namespace riffra::test
