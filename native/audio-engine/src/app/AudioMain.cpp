#include <JuceHeader.h>

#include <cstdint>
#include <iostream>
#include <limits>
#include <optional>

#include "app/AudioEngine.h"
#include "device/AudioConfiguration.h"
#include "device/AudioDeviceService.h"
#include "protocol/AudioProtocol.h"

namespace {

using riffra::AudioConfiguration;
using riffra::AudioDeviceService;
using riffra::AudioEngine;

/// Writes the one JSON line produced by a probe invocation.
void writeProbeResult(const juce::var& value) {
    std::cout << juce::JSON::toString(value, true) << std::endl;
}

/// Rejects invalid process arguments before any protocol output exists.
int rejectArguments(const juce::String& message) {
    std::cerr << message << std::endl;
    return 1;
}

/// Reports a --serve startup failure as a protocol fault.
int failServe(const juce::String& message) {
    riffra::writeEvent(riffra::FaultSpec{{"arguments", message, "sidecar.start", {}}});
    return 1;
}

int runMain(const juce::StringArray& arguments) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (arguments.size() < 2) {
        return rejectArguments("Use --probe, --probe-channels or --serve.");
    }
    const auto command = arguments[1];
    if (command == "--probe") {
        writeProbeResult(AudioDeviceService::discover());
        return 0;
    }
    if (command == "--probe-channels") {
        const auto findFlag = [&](const juce::String& flag) -> int {
            return arguments.indexOf(flag, false, 2);
        };
        const auto secondDriver = findFlag("--audio-driver");
        const auto secondInput = findFlag("--input-device");
        const auto secondOutput = findFlag("--output-device");
        const auto readValue = [&](const int flagIndex) -> juce::String {
            if (flagIndex < 2 || flagIndex + 1 >= arguments.size()) return {};
            return arguments[flagIndex + 1];
        };
        const auto driver = readValue(secondDriver);
        const auto inputDevice = readValue(secondInput);
        const auto outputDevice = readValue(secondOutput);
        if (driver.isEmpty()) {
            return rejectArguments("--probe-channels requires --audio-driver.");
        }
        juce::String probeError;
        const auto channels =
            AudioDeviceService::probeDeviceChannels(driver, inputDevice, outputDevice, probeError);
        if (!channels.has_value()) {
            return rejectArguments(probeError);
        }
        writeProbeResult(*channels);
        return 0;
    }
    if (command == "--serve") {
        std::optional<std::uint32_t> parentPid;
        AudioConfiguration configuration;
        for (int index = 2; index < arguments.size(); ++index) {
            const auto argument = arguments[index];
            if (argument != "--parent-pid" && argument != "--audio-driver" &&
                argument != "--input-device" && argument != "--input-channel" &&
                argument != "--output-device" && argument != "--sample-rate" &&
                argument != "--buffer-size")
                continue;
            if (index + 1 >= arguments.size()) {
                return failServe(argument + " requires a value.");
            }
            const auto value = arguments[++index];
            if (argument == "--parent-pid") {
                const auto pid = value.getLargeIntValue();
                if (pid <= 0 || pid > std::numeric_limits<std::uint32_t>::max()) {
                    return failServe("--parent-pid must be a positive process id.");
                }
                parentPid = static_cast<std::uint32_t>(pid);
            } else if (argument == "--audio-driver") {
                configuration.driver = value;
            } else if (argument == "--input-device") {
                configuration.inputDevice = value;
            } else if (argument == "--input-channel") {
                configuration.inputChannel = value.getIntValue();
                if (configuration.inputChannel < 0) {
                    return failServe("--input-channel must be zero or greater.");
                }
            } else if (argument == "--output-device") {
                configuration.outputDevice = value;
            } else if (argument == "--sample-rate") {
                configuration.sampleRate = value.getDoubleValue();
            } else if (argument == "--buffer-size") {
                configuration.bufferSize = value.getIntValue();
            }
        }
        AudioEngine engine;
        return engine.serve(parentPid, configuration);
    }
    return rejectArguments("Unknown command: " + command);
}

}  // namespace

#if JUCE_WINDOWS
int wmain(int argc, wchar_t* argv[]) {
    juce::StringArray arguments;
    for (int index = 0; index < argc; ++index) arguments.add(argv[index]);
    return runMain(arguments);
}
#else
int main(int argc, char* argv[]) {
    juce::StringArray arguments;
    for (int index = 0; index < argc; ++index) arguments.add(juce::String::fromUTF8(argv[index]));
    return runMain(arguments);
}
#endif
