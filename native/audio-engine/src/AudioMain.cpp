#include <JuceHeader.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdint>
#include <cstdlib>
#include <deque>
#include <exception>
#include <iostream>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

#include "AudioDeviceService.h"
#include "AudioProtocol.h"
#include "AudioRuntimeStatus.h"
#include "FaultInjection.h"
#include "MidiInputService.h"
#include "PluginEditorHost.h"
#include "RuntimeLifecycleExecutor.h"
#include "SafetyAudioCallback.h"
#include "TimelineEngine.h"

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

namespace {

using riffra::AudioConfiguration;
using riffra::AudioDeviceService;
using riffra::clearCurrentRequestId;
using riffra::currentRequestId;
using riffra::DeviceFaultWatcher;
using riffra::makeError;
using riffra::MidiInputService;
using riffra::OutputKind;
using riffra::parseMidiBytes;
using riffra::PluginEditorHost;
using riffra::RuntimeLifecycleExecutor;
using riffra::SafetyAudioCallback;
using riffra::setCurrentRequestId;
using riffra::TimelineEngine;
using riffra::writeJson;

constexpr auto kTimelineVstLifecycleTimeout = std::chrono::seconds(45);

bool parentProcessIsAlive(const std::uint32_t parentPid) noexcept {
#if JUCE_WINDOWS
    const auto process = OpenProcess(SYNCHRONIZE, FALSE, static_cast<DWORD>(parentPid));
    if (process == nullptr) return false;
    const auto result = WaitForSingleObject(process, 0);
    CloseHandle(process);
    return result == WAIT_TIMEOUT;
#else
    juce::ignoreUnused(parentPid);
    return true;
#endif
}

int serve(const std::optional<std::uint32_t> parentPid,
          const AudioConfiguration& startupConfiguration) {
    juce::AudioDeviceManager manager;
    juce::AudioFormatManager formatManager;
    formatManager.registerBasicFormats();
    TimelineEngine timelineEngine;
    SafetyAudioCallback callback;
    std::shared_ptr<PluginEditorHost> trackPluginEditor;
    juce::String trackPluginEditorTrackId;
    juce::String trackPluginEditorDeviceId;
    juce::AudioBuffer<float> comparisonRaw;
    juce::AudioBuffer<float> comparisonProcessed;
    MidiInputService midiInputs(callback, timelineEngine);
    callback.setTimelineEngine(&timelineEngine);
    callback.setEmergencyMuted(true);

    auto error = AudioDeviceService::initialise(manager, startupConfiguration);
    juce::String startupMessage;
    if (error.isNotEmpty()) {
        const auto requestedError = error;
        manager.closeAudioDevice();
#if JUCE_WINDOWS
        AudioConfiguration sharedFallback;
        sharedFallback.driver = "Windows Audio (Low Latency Mode)";
        error = AudioDeviceService::initialise(manager, sharedFallback);
        if (error.isNotEmpty()) {
            manager.closeAudioDevice();
            sharedFallback.driver = "Windows Audio";
            error = AudioDeviceService::initialise(manager, sharedFallback);
        }
        if (error.isNotEmpty()) {
            writeJson(makeError("audioDevice",
                                requestedError + ". Shared Windows audio also failed: " + error));
            return 2;
        }
        startupMessage =
            "The saved audio device was unavailable, so Riffra started with shared Windows audio.";
#else
        writeJson(makeError("audioDevice", requestedError));
        return 2;
#endif
    }

    auto startupInputChannel = startupMessage.isEmpty() ? startupConfiguration.inputChannel : 0;
    const auto startupInputChannels =
        manager.getCurrentAudioDevice() != nullptr
            ? manager.getCurrentAudioDevice()->getActiveInputChannels().countNumberOfSetBits()
            : 0;
    if (startupInputChannel >= startupInputChannels) {
        startupInputChannel = 0;
        startupMessage = "The saved input channel was unavailable, so Input 1 was selected.";
    }
    callback.setInputChannel(startupInputChannel);
    manager.addAudioCallback(&callback);
    DeviceFaultWatcher deviceWatcher(manager, callback, timelineEngine);
    manager.addChangeListener(&deviceWatcher);
    writeJson(AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor(),
                                                startupMessage));

    std::atomic<bool> watchdogRunning{true};
    std::thread watchdog;
    if (parentPid.has_value()) {
        watchdog = std::thread([&watchdogRunning, parentPid] {
            while (watchdogRunning.load(std::memory_order_acquire)) {
                std::this_thread::sleep_for(std::chrono::seconds(1));
                if (!watchdogRunning.load(std::memory_order_acquire)) break;
                if (!parentProcessIsAlive(*parentPid)) std::_Exit(0);
            }
        });
    }

    std::atomic<bool> midiPollRunning{true};
    std::thread midiPollThread([&] {
        while (midiPollRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
            if (!midiPollRunning.load(std::memory_order_acquire)) break;
            if (!midiInputs.isListening()) continue;
            if (midiInputs.deviceSetChanged()) {
                midiInputs.reopenAll();
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
            }
        }
    });

    // Meter push thread: periodically writes peak/dropout meters to stdout so
    // the Rust supervisor can emit compact audio-meter events to the frontend without
    // React polling. 50 ms ≈ 20 fps, smooth enough for meter UI.
    std::atomic<bool> meterPushRunning{true};
    std::thread meterPushThread([&] {
        while (meterPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!meterPushRunning.load(std::memory_order_acquire)) break;
            writeJson(AudioDeviceService::currentMeters(callback), {}, OutputKind::telemetry);
        }
    });

    std::atomic<bool> transportPushRunning{true};
    std::thread transportPushThread([&] {
        while (transportPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!transportPushRunning.load(std::memory_order_acquire)) break;
            writeJson(timelineEngine.status(), {}, OutputKind::telemetry);
        }
    });

    // Plugin construction and timeline preparation may execute third-party
    // code for an unbounded amount of time. Keep that work away from the
    // command reader so transport and workspace commands remain serviceable.
    std::atomic<bool> timelineOperationRunning{false};
    RuntimeLifecycleExecutor runtimeLifecycle([](RuntimeLifecycleExecutor::Task task) {
        if (!juce::MessageManager::callSync([task = std::move(task)]() mutable { task(); }))
            std::_Exit(125);
    });
    runtimeLifecycle.setTimeoutHandler([] {
        // Do not write to stdout here. The parent may be the stalled party or
        // its pipe may already be back-pressured; the watchdog's only bounded
        // operation is to terminate the isolated process so the Rust
        // supervisor can restart it in emergency-mute state.
        std::_Exit(124);
    });

    std::thread commandThread([&] {
        std::string line;
        while (std::getline(std::cin, line)) {
            clearCurrentRequestId();
            const auto command = juce::JSON::parse(juce::String::fromUTF8(line.c_str()));
            if (!command.isObject()) {
                writeJson(makeError("protocol", "Expected one JSON object per line."));
                continue;
            }

            setCurrentRequestId(command.getProperty("requestId", {}).toString());
            const auto type = command.getProperty("type", {}).toString();
            if (type == "shutdown") {
                callback.setEmergencyMuted(true);
                const auto submitted = runtimeLifecycle.submit(
                    [&] {
                        if (trackPluginEditor != nullptr) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                        }
                        timelineOperationRunning.store(false, std::memory_order_release);
                    },
                    std::chrono::seconds(10));
                if (submitted && !runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
                    std::_Exit(125);
                break;
            }
            if (type == "setEmergencyMute") {
                const auto muted = static_cast<bool>(command.getProperty("muted", true));
                if (!muted && callback.isDeviceFaulted()) {
                    writeJson(AudioDeviceService::currentStatus(manager, callback,
                                                                &midiInputs.monitor()));
                    continue;
                }
                callback.setEmergencyMuted(muted);
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "setMasterGainDb") {
                callback.setMasterGainDb(
                    static_cast<float>(command.getProperty("gainDb", callback.getMasterGainDb())));
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "loadTimelineSnapshot" || type == "prepareTimelineSnapshot") {
                if (static_cast<int>(command.getProperty("protocolVersion", 0)) != 1) {
                    writeJson(
                        makeError("timelineProtocol", "Unsupported timeline protocol version."));
                    continue;
                }
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "Another Arrangement Graph is still loading a VST3. The "
                                        "current runtime remains available."));
                    continue;
                }
                const auto commitImmediately = type == "loadTimelineSnapshot";
                auto* device = manager.getCurrentAudioDevice();
                const auto blockSize =
                    device != nullptr ? device->getCurrentBufferSizeSamples() : 0;
                const auto snapshot = command.getProperty("snapshot", {});
                const auto sampleRate = callback.getSampleRate();
                const auto requestId = currentRequestId();
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, snapshot, requestId, sampleRate, blockSize, commitImmediately] {
                        if (commitImmediately && trackPluginEditor != nullptr) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                        }
                        juce::String timelineError;
                        bool loaded = false;
                        try {
                            loaded = timelineEngine.loadSnapshot(snapshot, formatManager,
                                                                 sampleRate, blockSize,
                                                                 timelineError, commitImmediately);
                        } catch (const std::exception& exception) {
                            timelineError = "Arrangement VST3 loading raised an exception: " +
                                            juce::String(exception.what());
                        } catch (...) {
                            timelineError =
                                "Arrangement VST3 loading failed with an unknown exception.";
                        }
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!loaded) {
                            writeJson(makeError("timeline", timelineError), requestId);
                        } else {
                            auto* ack = new juce::DynamicObject();
                            ack->setProperty("type", "timelineAck");
                            ack->setProperty("revision", snapshot.getProperty("revision", 0));
                            ack->setProperty(
                                "appliedAtAudioClockSample",
                                timelineEngine.status().getProperty("audioClockSample", 0));
                            ack->setProperty("unavailableClipIds",
                                             snapshot.getProperty("unavailableClipIds",
                                                                  juce::Array<juce::var>{}));
                            writeJson(juce::var(ack), requestId);
                        }
                    },
                    kTimelineVstLifecycleTimeout);
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "commitTimelineSnapshot") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3 and cannot "
                                        "be committed yet."));
                    continue;
                }
                const auto requestId = currentRequestId();
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId] {
                        const auto shouldCloseEditor =
                            timelineEngine.hasPreparedSnapshot() && trackPluginEditor != nullptr &&
                            !timelineEngine.preparedTrackReusesRuntimeDevices(
                                trackPluginEditorTrackId);
                        if (shouldCloseEditor) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                        }
                        juce::String timelineError;
                        const auto committed = timelineEngine.commitPreparedSnapshot(timelineError);
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!committed) {
                            writeJson(makeError("timeline", timelineError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    kTimelineVstLifecycleTimeout);
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "discardTimelineSnapshot") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3 and cannot "
                                        "be discarded yet."));
                    continue;
                }
                const auto requestId = currentRequestId();
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId] {
                        timelineEngine.discardPreparedSnapshot();
                        timelineOperationRunning.store(false, std::memory_order_release);
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(5));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "setTrackDeviceBypassed") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Track "
                                        "device changes can be retried shortly."));
                    continue;
                }
                const auto requestId = currentRequestId();
                const auto trackId = command.getProperty("trackId", {}).toString();
                const auto deviceId = command.getProperty("deviceId", {}).toString();
                const auto bypassed = static_cast<bool>(command.getProperty("bypassed", false));
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, trackId, deviceId, bypassed] {
                        juce::String deviceError;
                        const auto changed = timelineEngine.setDeviceBypassed(
                            trackId, deviceId, bypassed, deviceError);
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!changed) {
                            writeJson(makeError("trackDevice", deviceError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(10));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "setTrackDeviceParameter") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Track "
                                        "device changes can be retried shortly."));
                    continue;
                }
                const auto requestId = currentRequestId();
                const auto trackId = command.getProperty("trackId", {}).toString();
                const auto deviceId = command.getProperty("deviceId", {}).toString();
                const auto parameterIndex =
                    static_cast<int>(command.getProperty("parameterIndex", -1));
                const auto value = static_cast<float>(command.getProperty("value", 0.0));
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, trackId, deviceId, parameterIndex, value] {
                        juce::String deviceError;
                        const auto changed = timelineEngine.setDeviceParameter(
                            trackId, deviceId, parameterIndex, value, deviceError);
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!changed) {
                            writeJson(makeError("trackDevice", deviceError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(10));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "openTrackPluginEditor") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. The plugin "
                                        "editor can be opened when it finishes."));
                    continue;
                }
                const auto requestId = currentRequestId();
                const auto editorTrackId = command.getProperty("trackId", {}).toString();
                const auto editorDeviceId = command.getProperty("deviceId", {}).toString();
                timelineOperationRunning.store(true, std::memory_order_release);
                const auto submitted = runtimeLifecycle.submit(
                    [&, requestId, editorTrackId, editorDeviceId] {
                        auto* device = timelineEngine.findDevice(editorTrackId, editorDeviceId);
                        if (device == nullptr) {
                            timelineOperationRunning.store(false, std::memory_order_release);
                            writeJson(makeError("trackDevice", "Track Device was not found."),
                                      requestId);
                            return;
                        }
                        if (trackPluginEditor != nullptr) {
                            trackPluginEditor->close();
                            trackPluginEditor.reset();
                        }
                        trackPluginEditorTrackId = editorTrackId;
                        trackPluginEditorDeviceId = editorDeviceId;
                        trackPluginEditor = std::make_shared<PluginEditorHost>(
                            *device,
                            [&, editorTrackId, editorDeviceId](const juce::var& state) {
                                const auto stateCopy = state;
                                const auto stateKey =
                                    "track-state:" +
                                    (editorTrackId + ":" + editorDeviceId).toStdString();
                                // State events are best-effort latest-value updates. Capacity
                                // drops and shutdown must never become unbounded control errors.
                                (void)runtimeLifecycle.submitState(
                                    stateKey,
                                    [&, editorTrackId, editorDeviceId, stateCopy, stateKey] {
                                        juce::String mirrorError;
                                        if (!timelineEngine.mirrorEditorDeviceState(
                                                editorTrackId, editorDeviceId, stateCopy,
                                                mirrorError)) {
                                            writeJson(makeError("trackDevice", mirrorError));
                                        }
                                        auto* changed = new juce::DynamicObject();
                                        changed->setProperty("type", "trackPluginStateChanged");
                                        changed->setProperty("trackId", editorTrackId);
                                        changed->setProperty("deviceId", editorDeviceId);
                                        changed->setProperty(
                                            "parameterValues",
                                            stateCopy.getProperty("parameterValues",
                                                                  juce::Array<juce::var>{}));
                                        changed->setProperty(
                                            "stateData", stateCopy.getProperty("stateData", {}));
                                        changed->setProperty(
                                            "bypassed", stateCopy.getProperty("bypassed", false));
                                        writeJson(juce::var(changed), {}, OutputKind::state,
                                                  stateKey);
                                    },
                                    std::chrono::seconds(10));
                            },
                            [&, editorTrackId, editorDeviceId](const int parameterIndex,
                                                               const float value) {
                                const auto stateKey =
                                    "track-parameter:" +
                                    (editorTrackId + ":" + editorDeviceId).toStdString() + ":" +
                                    std::to_string(parameterIndex);
                                // State events are best-effort latest-value updates. Capacity
                                // drops and shutdown must never become unbounded control errors.
                                (void)runtimeLifecycle.submitState(
                                    stateKey,
                                    [&, editorTrackId, editorDeviceId, parameterIndex, value,
                                     stateKey] {
                                        juce::String mirrorError;
                                        if (!timelineEngine.mirrorEditorDeviceParameter(
                                                editorTrackId, editorDeviceId, parameterIndex,
                                                value, mirrorError)) {
                                            writeJson(makeError("trackDevice", mirrorError));
                                            return;
                                        }
                                        auto* changed = new juce::DynamicObject();
                                        changed->setProperty("type", "trackPluginParameterChanged");
                                        changed->setProperty("trackId", editorTrackId);
                                        changed->setProperty("deviceId", editorDeviceId);
                                        changed->setProperty("parameterIndex", parameterIndex);
                                        changed->setProperty("value", value);
                                        writeJson(juce::var(changed), {}, OutputKind::state,
                                                  stateKey);
                                    },
                                    std::chrono::seconds(10));
                            });
                        juce::String editorError;
                        bool opened = false;
                        try {
                            opened = trackPluginEditor->open(editorError);
                        } catch (const std::exception& exception) {
                            editorError = "Track VST3 editor opening raised an exception: " +
                                          juce::String(exception.what());
                        } catch (...) {
                            editorError =
                                "Track VST3 editor opening failed with an unknown exception.";
                        }
                        timelineOperationRunning.store(false, std::memory_order_release);
                        if (!opened) {
                            trackPluginEditor.reset();
                            trackPluginEditorTrackId.clear();
                            trackPluginEditorDeviceId.clear();
                            writeJson(makeError("pluginEditor", editorError), requestId);
                            return;
                        }
                        writeJson(timelineEngine.status(), requestId);
                    },
                    std::chrono::seconds(30));
                if (!submitted) {
                    timelineOperationRunning.store(false, std::memory_order_release);
                    writeJson(
                        makeError("runtimeLifecycle", "The VST lifecycle executor is stopping."));
                }
                continue;
            }
            if (type == "playTimeline") {
                timelineEngine.play();
                writeJson(timelineEngine.status());
                continue;
            }
            if (type == "stopTimeline") {
                timelineEngine.stop();
                if (static_cast<bool>(command.getProperty("reportStatus", true)))
                    writeJson(timelineEngine.status());
                continue;
            }
            if (type == "seekTimeline") {
                const auto tick = static_cast<std::uint64_t>(
                    static_cast<juce::int64>(command.getProperty("tick", 0)));
                timelineEngine.seekToTick(tick);
                writeJson(timelineEngine.status());
                continue;
            }
            if (type == "enableMidiListening") {
                midiInputs.setListening(true);
                midiInputs.reopenAll();
                midiInputs.monitor().setActive(true);
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "disableMidiListening") {
                midiInputs.setListening(false);
                midiInputs.monitor().setActive(false);
                callback.stopPreview();
                callback.allNotesOff();
                midiInputs.reopenAll();
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "startTakeComparison") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Take "
                                        "comparison can be retried shortly."));
                    continue;
                }
                const auto loadComparisonFile = [&](const juce::String& path,
                                                    const juce::int64 startFrame,
                                                    const juce::int64 endFrame,
                                                    juce::AudioBuffer<float>& target,
                                                    juce::String& loadError) {
                    std::unique_ptr<juce::AudioFormatReader> reader(
                        path.isEmpty() ? nullptr : formatManager.createReaderFor(juce::File(path)));
                    if (reader == nullptr || reader->lengthInSamples <= 0 ||
                        reader->lengthInSamples > std::numeric_limits<int>::max()) {
                        loadError = "Take comparison source is unavailable.";
                        return false;
                    }
                    if (startFrame < 0 || endFrame <= startFrame ||
                        endFrame > reader->lengthInSamples ||
                        endFrame - startFrame > std::numeric_limits<int>::max()) {
                        loadError = "Take comparison range is outside its source.";
                        return false;
                    }
                    const auto sourceFrames = static_cast<int>(endFrame - startFrame);
                    const auto targetRate = callback.getSampleRate();
                    if (targetRate <= 0.0 || reader->sampleRate <= 0.0) {
                        loadError = "Take comparison requires an active output sample rate.";
                        return false;
                    }
                    juce::AudioBuffer<float> source(static_cast<int>(reader->numChannels),
                                                    sourceFrames + 4);
                    source.clear();
                    if (!reader->read(&source, 0, sourceFrames, startFrame, true, true)) {
                        loadError = "Take comparison source could not be read.";
                        return false;
                    }
                    const auto targetFrames = std::max(
                        1, static_cast<int>(std::llround(static_cast<double>(sourceFrames) *
                                                         targetRate / reader->sampleRate)));
                    target.setSize(static_cast<int>(reader->numChannels), targetFrames);
                    if (std::abs(reader->sampleRate - targetRate) <= 0.5) {
                        target.copyFrom(0, 0, source, 0, 0, targetFrames);
                        for (int channel = 1; channel < target.getNumChannels(); ++channel)
                            target.copyFrom(channel, 0, source, channel, 0, targetFrames);
                    } else {
                        const auto ratio = reader->sampleRate / targetRate;
                        for (int channel = 0; channel < target.getNumChannels(); ++channel) {
                            juce::LagrangeInterpolator interpolator;
                            interpolator.process(ratio, source.getReadPointer(channel),
                                                 target.getWritePointer(channel), targetFrames);
                        }
                    }
                    return true;
                };
                juce::String comparisonError;
                if (!loadComparisonFile(
                        command.getProperty("rawPath", {}).toString(),
                        static_cast<juce::int64>(command.getProperty("rawStartFrame", 0)),
                        static_cast<juce::int64>(command.getProperty("rawEndFrame", 0)),
                        comparisonRaw, comparisonError) ||
                    !loadComparisonFile(
                        command.getProperty("processedPath", {}).toString(),
                        static_cast<juce::int64>(command.getProperty("processedStartFrame", 0)),
                        static_cast<juce::int64>(command.getProperty("processedEndFrame", 0)),
                        comparisonProcessed, comparisonError) ||
                    !callback.startPreview(comparisonRaw, 0, comparisonRaw.getNumSamples(), 1.0f,
                                           false, comparisonError, 1)) {
                    writeJson(makeError("takeComparison", comparisonError));
                    continue;
                }
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "switchTakeComparisonVariant") {
                const auto variant = command.getProperty("variant", {}).toString();
                juce::String comparisonError;
                const auto& buffer = variant == "processed" ? comparisonProcessed : comparisonRaw;
                if (!callback.switchPreviewBuffer(1, buffer, comparisonError)) {
                    writeJson(makeError("takeComparison", comparisonError));
                    continue;
                }
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "stopTakeComparison") {
                callback.stopPreviewForKey(1);
                comparisonRaw.setSize(0, 0);
                comparisonProcessed.setSize(0, 0);
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "previewSample") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Preview "
                                        "can be retried shortly."));
                    continue;
                }
                const auto path = command.getProperty("path", {}).toString();
                std::unique_ptr<juce::AudioFormatReader> reader(
                    path.isEmpty() ? nullptr : formatManager.createReaderFor(juce::File(path)));
                juce::String previewError;
                const auto sampleRate = callback.getSampleRate();
                if (reader == nullptr) {
                    previewError = "Preview source could not be opened as an audio file.";
                } else if (sampleRate <= 0.0 || std::abs(reader->sampleRate - sampleRate) > 0.5) {
                    previewError =
                        "Preview source sample rate does not match the active audio device.";
                } else {
                    const auto length = std::min<juce::int64>(
                        reader->lengthInSamples,
                        static_cast<juce::int64>(std::numeric_limits<int>::max()));
                    juce::AudioBuffer<float> buffer(reader->numChannels, static_cast<int>(length));
                    if (length <= 0 ||
                        !reader->read(&buffer, 0, static_cast<int>(length), 0, true, true)) {
                        previewError = "Preview source contains no readable audio samples.";
                    } else {
                        const auto startMs =
                            static_cast<double>(command.getProperty("startMs", 0.0));
                        const auto endMs = static_cast<double>(command.getProperty("endMs", -1.0));
                        const auto start = juce::jlimit(
                            0, static_cast<int>(length),
                            static_cast<int>(std::llround(startMs * reader->sampleRate / 1000.0)));
                        const auto end =
                            endMs <= 0.0 ? static_cast<int>(length)
                                         : juce::jlimit(start + 1, static_cast<int>(length),
                                                        static_cast<int>(std::llround(
                                                            endMs * reader->sampleRate / 1000.0)));
                        if (!callback.startPreview(
                                buffer, start, end,
                                static_cast<float>(
                                    static_cast<double>(command.getProperty("gain", 1.0))),
                                static_cast<bool>(command.getProperty("loop", false)), previewError,
                                -1))
                            previewError =
                                previewError.isEmpty() ? "Preview range is invalid." : previewError;
                    }
                }
                if (previewError.isNotEmpty()) {
                    writeJson(makeError("preview", previewError));
                    continue;
                }
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "stopPreview") {
                callback.stopPreview();
                callback.allNotesOff();
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "sendTrackMidi" || type == "panicTrackMidi") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still changing; targeted MIDI "
                                        "can be retried shortly."));
                    continue;
                }
                const auto trackId = command.getProperty("trackId", {}).toString();
                juce::String timelineError;
                bool accepted = false;
                if (type == "sendTrackMidi") {
                    juce::MidiMessage message;
                    juce::String midiError;
                    if (!parseMidiBytes(command.getProperty("bytes", {}), message, midiError)) {
                        writeJson(makeError("midi", midiError));
                        continue;
                    }
                    accepted = timelineEngine.enqueueTargetedMidi(trackId, message, timelineError);
                } else {
                    accepted = timelineEngine.panicTargetedMidi(trackId, timelineError);
                }
                if (!accepted) {
                    writeJson(makeError("targetedMidi", timelineError));
                    continue;
                }
                writeJson(AudioDeviceService::currentMeters(callback));
                continue;
            }
            if (type == "recoverAudioDevice") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Audio "
                                        "device recovery can be retried shortly."));
                    continue;
                }
                juce::AudioDeviceManager::AudioDeviceSetup recoverySetup;
                manager.getAudioDeviceSetup(recoverySetup);
                manager.removeAudioCallback(&callback);
                manager.closeAudioDevice();
                callback.setEmergencyMuted(true);
                const auto recoveryError = manager.setAudioDeviceSetup(recoverySetup, true);
                if (recoveryError.isNotEmpty()) {
                    writeJson(makeError("audioDevice", recoveryError));
                    continue;
                }
                manager.addAudioCallback(&callback);
                callback.setDeviceFaulted(false);
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "setAudioDriver") {
                if (timelineOperationRunning.load(std::memory_order_acquire)) {
                    writeJson(makeError("timelineBusy",
                                        "The Arrangement Graph is still loading a VST3. Audio "
                                        "driver changes can be retried shortly."));
                    continue;
                }
                const auto driver = command.getProperty("driver", {}).toString();
                if (driver.isEmpty()) {
                    writeJson(makeError("audioDevice", "An audio driver name is required."));
                    continue;
                }
                AudioConfiguration requested;
                requested.driver = driver;
                requested.inputDevice = command.getProperty("inputDevice", {}).toString();
                requested.outputDevice = command.getProperty("outputDevice", {}).toString();
                requested.inputChannel = static_cast<int>(command.getProperty("inputChannel", 0));
                if (requested.inputChannel < 0) {
                    writeJson(makeError("audioDevice", "Input channel must be zero or greater."));
                    continue;
                }
                requested.sampleRate = static_cast<double>(command.getProperty("sampleRate", 0.0));
                requested.bufferSize = static_cast<int>(command.getProperty("bufferSize", 0));
                const auto previousDriver = manager.getCurrentAudioDeviceType();
                const auto previousInputChannel = callback.getInputChannel();
                juce::AudioDeviceManager::AudioDeviceSetup previousSetup;
                manager.getAudioDeviceSetup(previousSetup);
                manager.removeAudioCallback(&callback);
                manager.closeAudioDevice();
                callback.setEmergencyMuted(true);
                const auto restorePreviousDevice = [&]() {
                    manager.closeAudioDevice();
                    AudioConfiguration previous;
                    previous.driver = previousDriver;
                    previous.inputDevice = previousSetup.inputDeviceName;
                    previous.outputDevice = previousSetup.outputDeviceName;
                    previous.inputChannel = previousInputChannel;
                    previous.sampleRate = previousSetup.sampleRate;
                    previous.bufferSize = previousSetup.bufferSize;
                    const auto restoreError = AudioDeviceService::initialise(manager, previous);
                    if (restoreError.isEmpty()) {
                        callback.setInputChannel(previousInputChannel);
                        manager.addAudioCallback(&callback);
                        callback.setDeviceFaulted(false);
                    }
                    return restoreError;
                };
                auto setupError = AudioDeviceService::initialise(manager, requested);
                if (setupError.isNotEmpty()) {
                    const auto restoreError = restorePreviousDevice();
                    writeJson(makeError(
                        "audioDevice",
                        setupError + (restoreError.isEmpty()
                                          ? ". The previous device was restored."
                                          : ". The previous device could not be restored: " +
                                                restoreError)));
                    continue;
                }
                auto* activeDevice = manager.getCurrentAudioDevice();
                const auto activeInputs =
                    activeDevice != nullptr
                        ? activeDevice->getActiveInputChannels().countNumberOfSetBits()
                        : 0;
                if (requested.inputChannel >= activeInputs) {
                    const auto restoreError = restorePreviousDevice();
                    const auto message =
                        juce::String("The selected physical input channel is unavailable.") +
                        (restoreError.isEmpty()
                             ? " The previous device was restored."
                             : " The previous device could not be restored: " + restoreError);
                    writeJson(makeError("audioDevice", message));
                    continue;
                }
                callback.setInputChannel(requested.inputChannel);
                manager.addAudioCallback(&callback);
                callback.setDeviceFaulted(false);
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "startArrangeRecording") {
                const auto directory = command.getProperty("directory", {}).toString();
                juce::String recordingError;
                const auto started = callback.startArrangeRecording(juce::File(directory),
                                                                    timelineEngine, recordingError);
                if (directory.isEmpty() || !started) {
                    writeJson(makeError("recording", directory.isEmpty()
                                                         ? "Recording directory is required."
                                                         : recordingError));
                    continue;
                }
                if (!timelineEngine.startRecording(
                        static_cast<int>(command.getProperty("countInBeats", 0)), recordingError)) {
                    juce::String rollbackError;
                    (void)callback.stopArrangeRecording(timelineEngine, rollbackError);
                    writeJson(makeError("recording", recordingError));
                    continue;
                }
                writeJson(AudioDeviceService::currentStatus(
                    manager, callback, &midiInputs.monitor(), {}, &timelineEngine));
                continue;
            }
            if (type == "stopArrangeRecording") {
                const auto cancelledCountIn = timelineEngine.cancelRecordingIfCountingIn();

                juce::String recordingError;

                if (cancelledCountIn) {
                    timelineEngine.stop();

                    if (!callback.cancelArrangeRecording(timelineEngine, recordingError)) {
                        writeJson(makeError("recording", recordingError));
                        continue;
                    }
                } else {
                    if (!callback.stopArrangeRecording(timelineEngine, recordingError)) {
                        writeJson(makeError("recording", recordingError));
                        continue;
                    }

                    timelineEngine.stop();
                }

                writeJson(AudioDeviceService::currentStatus(
                    manager, callback, &midiInputs.monitor(), {}, &timelineEngine));
                continue;
            }
            if (type == "status") {
                writeJson(
                    AudioDeviceService::currentStatus(manager, callback, &midiInputs.monitor()));
                continue;
            }
            if (type == "meterStatus") {
                writeJson(AudioDeviceService::currentMeters(callback));
                continue;
            }
            writeJson(makeError("protocol", "Unsupported command: " + type));
        }

        const auto cleanupSubmitted = runtimeLifecycle.submit(
            [&] {
                if (trackPluginEditor != nullptr) {
                    trackPluginEditor->close();
                    trackPluginEditor.reset();
                    trackPluginEditorTrackId.clear();
                    trackPluginEditorDeviceId.clear();
                }
                timelineOperationRunning.store(false, std::memory_order_release);
            },
            std::chrono::seconds(10));
        if (cleanupSubmitted && !runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500)))
            std::_Exit(125);
        juce::MessageManager::callAsync(
            [] { juce::MessageManager::getInstance()->stopDispatchLoop(); });
    });

    juce::MessageManager::getInstance()->runDispatchLoop();
    if (commandThread.joinable()) commandThread.join();
    if (!runtimeLifecycle.waitForIdle(std::chrono::milliseconds(1500))) std::_Exit(125);
    runtimeLifecycle.requestStop();
    runtimeLifecycle.join();

    callback.setEmergencyMuted(true);
    midiInputs.monitor().setActive(false);
    midiInputs.setListening(false);
    midiInputs.reopenAll();
    manager.removeAudioCallback(&callback);
    manager.removeChangeListener(&deviceWatcher);
    manager.closeAudioDevice();
    watchdogRunning.store(false, std::memory_order_release);
    if (watchdog.joinable()) watchdog.join();
    midiPollRunning.store(false, std::memory_order_release);
    if (midiPollThread.joinable()) midiPollThread.join();
    meterPushRunning.store(false, std::memory_order_release);
    if (meterPushThread.joinable()) meterPushThread.join();
    transportPushRunning.store(false, std::memory_order_release);
    if (transportPushThread.joinable()) transportPushThread.join();
    return 0;
}

}  // namespace

int runMain(const juce::StringArray& arguments) {
    juce::ScopedJuceInitialiser_GUI juceInitialiser;
    if (arguments.size() < 2) {
        writeJson(makeError("arguments", "Use --probe, --probe-channels or --serve."));
        return 1;
    }
    const auto command = arguments[1];
    if (command == "--probe") {
        writeJson(AudioDeviceService::discover());
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
            writeJson(makeError("arguments", "--probe-channels requires --audio-driver."));
            return 1;
        }
        juce::String probeError;
        const auto channels =
            AudioDeviceService::probeDeviceChannels(driver, inputDevice, outputDevice, probeError);
        if (!channels.has_value()) {
            writeJson(makeError("deviceChannels", probeError));
            return 1;
        }
        writeJson(*channels);
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
                writeJson(makeError("arguments", argument + " requires a value."));
                return 1;
            }
            const auto value = arguments[++index];
            if (argument == "--parent-pid") {
                const auto pid = value.getLargeIntValue();
                if (pid <= 0 || pid > std::numeric_limits<std::uint32_t>::max()) {
                    writeJson(
                        makeError("arguments", "--parent-pid must be a positive process id."));
                    return 1;
                }
                parentPid = static_cast<std::uint32_t>(pid);
            } else if (argument == "--audio-driver") {
                configuration.driver = value;
            } else if (argument == "--input-device") {
                configuration.inputDevice = value;
            } else if (argument == "--input-channel") {
                configuration.inputChannel = value.getIntValue();
                if (configuration.inputChannel < 0) {
                    writeJson(makeError("arguments", "--input-channel must be zero or greater."));
                    return 1;
                }
            } else if (argument == "--output-device") {
                configuration.outputDevice = value;
            } else if (argument == "--sample-rate") {
                configuration.sampleRate = value.getDoubleValue();
            } else if (argument == "--buffer-size") {
                configuration.bufferSize = value.getIntValue();
            }
        }
        return serve(parentPid, configuration);
    }
    writeJson(makeError("arguments", "Unknown command: " + command));
    return 1;
}

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
