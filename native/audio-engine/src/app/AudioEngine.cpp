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

#include "app/AudioStatusBuilder.h"
#include "audio/AudioRenderPipeline.h"
#include "device/AudioDeviceController.h"
#include "device/AudioDeviceService.h"
#include "midi/MidiInputService.h"
#include "plugins/FaultInjection.h"
#include "plugins/PluginEditorHost.h"
#include "plugins/RuntimeLifecycleExecutor.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

#if JUCE_WINDOWS
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

namespace {
constexpr auto kTimelineVstLifecycleTimeout = std::chrono::seconds(45);
constexpr int kGraphReclaimIntervalMs = 100;

/// Destroys the graphs the audio thread has retired, on the message thread.
class GraphReclaimTimer final : public juce::Timer {
public:
    explicit GraphReclaimTimer(riffra::TimelineEngine& timelineIn) : timeline(timelineIn) {}

    void timerCallback() override { timeline.reclaimRetiredGraphs(); }

private:
    riffra::TimelineEngine& timeline;
};

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

    const auto currentStatus = [&] {
        return AudioStatusBuilder::currentStatus(manager, pipeline, midiInputs.monitor(),
                                                 timelineEngine);
    };
    auto error = deviceController.initialise(startupConfiguration);
    if (error.isNotEmpty()) {
        deviceController.close();
        writeEvent(FaultSpec{{"deviceRejected", error, "audioDevice.activate", {}}});
        return 2;
    }

    const auto startupInputChannels =
        manager.getCurrentAudioDevice() != nullptr
            ? manager.getCurrentAudioDevice()->getInputChannelNames().size()
            : 0;
    if (startupInputChannels > 0 && startupConfiguration.inputChannel >= startupInputChannels) {
        writeEvent(FaultSpec{{"deviceRejected", "The saved input channel is unavailable.",
                              "audioDevice.activate",
                              encodeInputChannelDetails(
                                  {static_cast<std::uint32_t>(startupConfiguration.inputChannel),
                                   static_cast<std::uint32_t>(startupInputChannels)})}});
        deviceController.close();
        return 2;
    }
    pipeline.setInputChannel(startupConfiguration.inputChannel);
    deviceController.setDeviceLossHandler([&] { writeEvent(currentStatus()); });
    deviceController.attach();
    writeEvent(ReadySpec{currentStatus()});

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
                writeEvent(currentStatus());
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
            writeEvent(AudioStatusBuilder::currentMeters(pipeline, timelineEngine));
        }
    });

    transportPushRunning.store(true, std::memory_order_release);
    transportPushThread = std::thread([&] {
        while (transportPushRunning.load(std::memory_order_acquire)) {
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            if (!transportPushRunning.load(std::memory_order_acquire)) break;
            writeEvent(AudioStatusBuilder::currentTransport(timelineEngine));
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
            const auto summary = session->summary();
            pipeline.completeArrangeRecordingProcessing(summary, error);
            writeEvent(RecordingCompleteSpec{
                summary.directory, processed && finished,
                error.isNotEmpty() ? std::optional<juce::String>(error) : std::nullopt});
            writeEvent(currentStatus());
        };

    pipeline.setRecordingFinalizationDispatcher([&](std::unique_ptr<riffra::ArrangeRecordingSession>
                                                        session,
                                                    const juce::String& finalizationError) {
        if (session == nullptr) return;
        auto owned = std::shared_ptr<riffra::ArrangeRecordingSession>(std::move(session));
        const auto submitted = runtimeLifecycle.submitWithProgress(
            [&, owned, finalizationError] {
                runtimeLifecycle.reportProgress();
                auto processingError = finalizationError;
                const auto processed =
                    processingError.isEmpty() &&
                    timelineEngine.processFinalizedRecording(
                        owned.get(), processingError, [&] { runtimeLifecycle.reportProgress(); });
                publishRecordingCompletion(owned, processed, processingError);
            },
            kRecordingFinalizationStallTimeout);
        if (!submitted) {
            publishRecordingCompletion(
                owned, false,
                finalizationError.isNotEmpty()
                    ? finalizationError
                    : "The recording finalization worker stopped before processing could begin.");
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

    GraphReclaimTimer graphReclaimTimer(timelineEngine);
    graphReclaimTimer.startTimer(kGraphReclaimIntervalMs);
    juce::MessageManager::getInstance()->runDispatchLoop();
    graphReclaimTimer.stopTimer();
    if (commandThread.joinable()) commandThread.join();
    if (!commandDispatcher->waitForBackgroundWork(std::chrono::milliseconds(1500))) std::_Exit(125);
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
