#include "AudioStatusBuilder.h"

#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "audio/AudioRenderPipeline.h"
#include "processing/TimelineEngine.h"

namespace riffra {
namespace {

juce::var midiDeviceValue(const juce::MidiDeviceInfo& device) {
    auto* value = new juce::DynamicObject();
    value->setProperty("id", device.identifier);
    value->setProperty("name", device.name);
    return juce::var(value);
}

}  // namespace

juce::var AudioStatusBuilder::currentStatus(juce::AudioDeviceManager& manager,
                                            const AudioRenderPipeline& pipeline,
                                            const MidiMonitor* midi, const juce::String& message,
                                            TimelineEngine* timeline) {
    auto* status = new juce::DynamicObject();
    status->setProperty("type", "audioStatus");
    const juce::String state =
        pipeline.isDeviceFaulted() ? "faulted" : (pipeline.isMuted() ? "muted" : "ready");
    status->setProperty("state", state);
    if (pipeline.isDeviceFaulted())
        status->setProperty(
            "message",
            "Audio device disconnected; output is muted and any captured take is preserved.");
    status->setProperty("muteReasons", static_cast<juce::int64>(pipeline.getMuteReasons()));
    status->setProperty("masterGainDb", pipeline.getMasterGainDb());
    status->setProperty("inputPeak", pipeline.getInputPeak());
    status->setProperty("outputPeak", pipeline.getOutputPeak());
    status->setProperty("invalidSamples",
                        static_cast<juce::int64>(pipeline.getInvalidSampleCount()));
    status->setProperty("feedbackSuspected", pipeline.isFeedbackSuspected());
    status->setProperty("previewing", pipeline.isPreviewing());
    if (midi != nullptr) {
        status->setProperty("midiInputActive", midi->isActive());
        status->setProperty("midiMessages", static_cast<juce::int64>(midi->getMessageCount()));
        status->setProperty("lastMidiNote", midi->getLastNote());
    }
    status->setProperty("recording", pipeline.recordingStatus());
    auto* diagnostics = new juce::DynamicObject();
    diagnostics->setProperty("callbackCount",
                             static_cast<juce::int64>(pipeline.getCallbackCount()));
    diagnostics->setProperty("averageCallbackDurationUs",
                             static_cast<juce::int64>(pipeline.getAverageCallbackDurationUs()));
    diagnostics->setProperty("maximumCallbackDurationUs",
                             static_cast<juce::int64>(pipeline.getMaximumCallbackDurationUs()));
    diagnostics->setProperty("callbackOverruns",
                             static_cast<juce::int64>(pipeline.getCallbackOverruns()));
    diagnostics->setProperty("preLimiterPeak", pipeline.getPreLimiterPeak());
    diagnostics->setProperty("limiterGainReductionDb", pipeline.getLimiterGainReductionDb());
    diagnostics->setProperty("hardClipSamples",
                             static_cast<juce::int64>(pipeline.getHardClipSamples()));
    if (timeline != nullptr) {
        timeline->serviceDeferredCleanup();
        const auto timelineStatus = timeline->status();
        status->setProperty("timelineTick", timelineStatus.getProperty("timelineTick", 0));
        diagnostics->setProperty("liveMidiDrops", timelineStatus.getProperty("liveMidiDrops", 0));
        diagnostics->setProperty("trackCount", timelineStatus.getProperty("trackCount", 0));
        diagnostics->setProperty("instrumentRuntimeCount",
                                 timelineStatus.getProperty("instrumentRuntimeCount", 0));
        diagnostics->setProperty("pluginCount", timelineStatus.getProperty("pluginCount", 0));
        diagnostics->setProperty("maximumLatencySamples",
                                 timelineStatus.getProperty("maximumLatencySamples", 0));
        diagnostics->setProperty("graphRevision", timelineStatus.getProperty("graphRevision", 0));
        diagnostics->setProperty("graphPublishCount",
                                 timelineStatus.getProperty("graphPublishCount", 0));
        diagnostics->setProperty(
            "instrumentFaults",
            timelineStatus.getProperty("instrumentFaults", juce::Array<juce::var>{}));
    }
    status->setProperty("diagnostics", juce::var(diagnostics));
    if (message.isNotEmpty()) status->setProperty("message", message);

    juce::Array<juce::var> midiInputs;
    for (const auto& device : juce::MidiInput::getAvailableDevices())
        midiInputs.add(midiDeviceValue(device));
    juce::Array<juce::var> midiOutputs;
    for (const auto& device : juce::MidiOutput::getAvailableDevices())
        midiOutputs.add(midiDeviceValue(device));
    status->setProperty("midiInputs", midiInputs);
    status->setProperty("midiOutputs", midiOutputs);

    if (auto* device = manager.getCurrentAudioDevice()) {
        juce::AudioDeviceManager::AudioDeviceSetup setup;
        manager.getAudioDeviceSetup(setup);
        status->setProperty("driver", device->getTypeName());
        status->setProperty("inputDevice", setup.inputDeviceName);
        status->setProperty("outputDevice", setup.outputDeviceName);
        status->setProperty("inputChannel", pipeline.getInputChannel());
        juce::Array<juce::var> inputChannels;
        const auto channelNames = device->getInputChannelNames();
        const auto activeInputChannels = device->getActiveInputChannels();
        juce::Array<juce::var> activeInputChannelIndices;
        for (int physicalIndex = 0; physicalIndex < channelNames.size(); ++physicalIndex) {
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", physicalIndex);
            channel->setProperty("name", channelNames[physicalIndex].isNotEmpty()
                                             ? channelNames[physicalIndex]
                                             : "Input " + juce::String(physicalIndex + 1));
            inputChannels.add(juce::var(channel));
            if (activeInputChannels[physicalIndex]) activeInputChannelIndices.add(physicalIndex);
        }
        status->setProperty("inputChannels", inputChannels);
        status->setProperty("activeInputChannels", activeInputChannelIndices);
        juce::Array<juce::var> outputChannels;
        const auto outputChannelNames = device->getOutputChannelNames();
        const auto activeOutputChannels = device->getActiveOutputChannels();
        juce::Array<juce::var> activeOutputChannelIndices;
        for (int physicalIndex = 0; physicalIndex < outputChannelNames.size(); ++physicalIndex) {
            auto* channel = new juce::DynamicObject();
            channel->setProperty("index", physicalIndex);
            channel->setProperty("name", outputChannelNames[physicalIndex].isNotEmpty()
                                             ? outputChannelNames[physicalIndex]
                                             : "Output " + juce::String(physicalIndex + 1));
            outputChannels.add(juce::var(channel));
            if (activeOutputChannels[physicalIndex]) activeOutputChannelIndices.add(physicalIndex);
        }
        status->setProperty("outputChannels", outputChannels);
        status->setProperty("activeOutputChannels", activeOutputChannelIndices);
        status->setProperty("sampleRate", device->getCurrentSampleRate());
        status->setProperty("bufferSize", device->getCurrentBufferSizeSamples());
        const auto latencySamples =
            device->getInputLatencyInSamples() + device->getOutputLatencyInSamples();
        const auto latencyMs =
            device->getCurrentSampleRate() > 0.0
                ? 1000.0 * static_cast<double>(latencySamples) / device->getCurrentSampleRate()
                : 0.0;
        status->setProperty("roundTripMs", latencyMs);
    }
    return juce::var(status);
}

juce::var AudioStatusBuilder::currentMeters(const AudioRenderPipeline& pipeline) {
    auto* meters = new juce::DynamicObject();
    meters->setProperty("type", "audioMeters");
    meters->setProperty("inputPeak", pipeline.getInputPeak());
    meters->setProperty("outputPeak", pipeline.getOutputPeak());
    meters->setProperty("invalidSamples",
                        static_cast<juce::int64>(pipeline.getInvalidSampleCount()));
    meters->setProperty("preLimiterPeak", pipeline.getPreLimiterPeak());
    meters->setProperty("limiterGainReductionDb", pipeline.getLimiterGainReductionDb());
    meters->setProperty("hardClipSamples", static_cast<juce::int64>(pipeline.getHardClipSamples()));
    meters->setProperty("muteReasons", static_cast<juce::int64>(pipeline.getMuteReasons()));
    meters->setProperty("feedbackSuspected", pipeline.isFeedbackSuspected());
    meters->setProperty("previewing", pipeline.isPreviewing());
    meters->setProperty("droppedTelemetryFrames",
                        static_cast<juce::int64>(droppedTelemetryCount()));
    meters->setProperty("droppedStateEvents", static_cast<juce::int64>(droppedStateCount()));
    return juce::var(meters);
}

}  // namespace riffra
