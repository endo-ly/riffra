#include "AudioEngine.h"

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

#include "AudioProtocol.h"
#include "FaultInjection.h"
#include "MidiInputService.h"
#include "PluginEditorHost.h"
#include "RuntimeLifecycleExecutor.h"
#include "TimelineEngine.h"
#include "app/AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceController.h"
#include "device/AudioDeviceService.h"

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

namespace {
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

}  // namespace

namespace riffra {

AudioEngine::AudioEngine()
    : pipeline(timelineEngine),
      deviceController(pipeline,
                       [this] {
                           juce::MessageManager::callAsync([this] {
                               juce::String ignored;
                               (void)pipeline.recording().stop(ignored);
                           });
                       }),
      midiInputs(pipeline.preview(), timelineEngine),
      runtimeLifecycle([](RuntimeLifecycleExecutor::Task task) {
          if (!juce::MessageManager::callSync([task = std::move(task)]() mutable { task(); }))
              std::_Exit(125);
      }) {
    commandDispatcher = std::make_unique<AudioCommandDispatcher>(AudioCommandDispatcher::Context{
        formatManager, timelineEngine, pipeline, deviceController, midiInputs, runtimeLifecycle,
        trackPluginEditor, trackPluginEditorTrackId, trackPluginEditorDeviceId, comparisonRaw,
        comparisonProcessed, timelineOperationRunning});
}

AudioEngine::~AudioEngine() = default;

int AudioEngine::serve(const std::optional<std::uint32_t> parentPid,
                       const AudioConfiguration& startupConfiguration) {
    formatManager.registerBasicFormats();
    pipeline.setEngineTransitionMute(true);
    auto& manager = deviceController.manager();

    auto error = deviceController.initialise(startupConfiguration);
    juce::String startupMessage;
    if (error.isNotEmpty()) {
        deviceController.close();
        writeJson(makeError("deviceRejected", error, "audioDevice.activate"));
        return 2;
    }

    auto startupInputChannel = startupMessage.isEmpty() ? startupConfiguration.inputChannel : 0;
    const auto startupInputChannels =
        manager.getCurrentAudioDevice() != nullptr
            ? manager.getCurrentAudioDevice()->getInputChannelNames().size()
            : 0;
    if (startupInputChannels > 0 && startupInputChannel >= startupInputChannels) {
        auto* details = new juce::DynamicObject();
        details->setProperty("inputChannel", startupInputChannel);
        details->setProperty("availableInputChannels", startupInputChannels);
        writeJson(makeError("deviceRejected", "The saved input channel is unavailable.",
                            "audioDevice.activate", juce::var(details)));
        deviceController.close();
        return 2;
    }
    pipeline.setInputChannel(startupInputChannel);
    deviceController.setDeviceLossHandler([&] {
        writeJson(
            AudioStatusBuilder::currentStatus(manager, pipeline, nullptr, {}, &timelineEngine));
    });
    deviceController.attach();
    writeJson(AudioStatusBuilder::currentStatus(manager, pipeline, &midiInputs.monitor(),
                                                startupMessage, &timelineEngine));

    watchdogRunning.store(true, std::memory_order_release);
    if (parentPid.has_value()) {
        watchdog = std::thread([this, parentPid] {
            while (watchdogRunning.load(std::memory_order_acquire)) {
                std::this_thread::sleep_for(std::chrono::seconds(1));
                if (!watchdogRunning.load(std::memory_order_acquire)) break;
                if (!parentProcessIsAlive(*parentPid)) std::_Exit(0);
            }
        });
    }

    midiPollRunning.store(true, std::memory_order_release);
    midiPollThread = std::thread([&] {
        while (midiPollRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
            if (!midiPollRunning.load(std::memory_order_acquire)) break;
            if (!midiInputs.isListening()) continue;
            if (midiInputs.deviceSetChanged()) {
                midiInputs.reopenAll();
                writeJson(AudioStatusBuilder::currentStatus(
                    manager, pipeline, &midiInputs.monitor(), {}, &timelineEngine));
            }
        }
    });

    // Meter push thread: periodically writes peak/dropout meters to stdout so
    // the Rust supervisor can emit compact audio-meter events to the frontend without
    // React polling. 50 ms ≈ 20 fps, smooth enough for meter UI.
    meterPushRunning.store(true, std::memory_order_release);
    meterPushThread = std::thread([&] {
        while (meterPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!meterPushRunning.load(std::memory_order_acquire)) break;
            writeJson(AudioStatusBuilder::currentMeters(pipeline), {}, OutputKind::telemetry);
        }
    });

    transportPushRunning.store(true, std::memory_order_release);
    transportPushThread = std::thread([&] {
        while (transportPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!transportPushRunning.load(std::memory_order_acquire)) break;
            writeJson(timelineEngine.status(), {}, OutputKind::telemetry);
        }
    });

    // Plugin construction and timeline preparation may execute third-party
    // code for an unbounded amount of time. Keep that work away from the
    // command reader so transport and workspace commands remain serviceable.
    constexpr auto kRecordingFinalizationStallTimeout = std::chrono::seconds(45);
    runtimeLifecycle.setTimeoutHandler([] {
        // Do not write to stdout here. The parent may be the stalled party or
        // its pipe may already be back-pressured; the watchdog's only bounded
        // operation is to terminate the isolated process so the Rust
        // supervisor can restart it in emergency-mute state.
        std::_Exit(124);
    });

    const auto publishRecordingCompletion =
        [&](const std::shared_ptr<riffra::ArrangeRecordingSession>& session, const bool processed,
            const juce::String& processingError) {
            juce::String finishError;
            const auto finished = session->finish(processed, finishError);
            juce::String error = processingError;
            if (finishError.isNotEmpty()) {
                if (error.isNotEmpty()) error << " ";
                error << finishError;
            }
            const auto succeeded = processed && finished;
            const auto status = session->status();
            pipeline.completeArrangeRecordingProcessing(status, error);

            auto* completion = new juce::DynamicObject();
            completion->setProperty("type", "recordingComplete");
            completion->setProperty("directory", status.getProperty("directory", {}));
            completion->setProperty("success", succeeded);
            if (error.isNotEmpty()) completion->setProperty("message", error);
            writeJson(juce::var(completion));
            writeJson(AudioStatusBuilder::currentStatus(manager, pipeline, &midiInputs.monitor(),
                                                        {}, &timelineEngine));
        };

    pipeline.setRecordingFinalizationDispatcher(
        [&](std::unique_ptr<riffra::ArrangeRecordingSession> session) {
            if (session == nullptr) return;
            auto owned = std::shared_ptr<riffra::ArrangeRecordingSession>(std::move(session));
            const auto submitted = runtimeLifecycle.submitWithProgress(
                [&, owned] {
                    runtimeLifecycle.reportProgress();
                    juce::String processingError;
                    const auto processed = timelineEngine.processFinalizedRecording(
                        owned.get(), processingError, [&] { runtimeLifecycle.reportProgress(); });
                    publishRecordingCompletion(owned, processed, processingError);
                },
                kRecordingFinalizationStallTimeout);
            if (!submitted) {
                publishRecordingCompletion(
                    owned, false,
                    "The recording finalization worker stopped before processing could begin.");
            }
        });

    std::thread commandThread([this] {
        commandDispatcher->run(std::cin);
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
    pipeline.setRecordingFinalizationDispatcher({});
    runtimeLifecycle.requestStop();
    runtimeLifecycle.join();

    pipeline.setEngineTransitionMute(true);
    midiInputs.monitor().setActive(false);
    midiInputs.setListening(false);
    midiInputs.reopenAll();
    deviceController.close();
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

}  // namespace riffra
