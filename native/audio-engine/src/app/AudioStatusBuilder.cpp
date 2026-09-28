#include "AudioStatusBuilder.h"

#include "audio/AudioRenderPipeline.h"
#include "midi/MidiInputService.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

std::vector<MidiDeviceSpec> midiDevices(const juce::Array<juce::MidiDeviceInfo>& devices) {
    std::vector<MidiDeviceSpec> result;
    result.reserve(static_cast<std::size_t>(devices.size()));
    for (const auto& device : devices) result.push_back({device.identifier, device.name});
    return result;
}

void describeChannels(const juce::StringArray& names, const juce::BigInteger& active,
                      const char* fallbackPrefix, std::vector<AudioChannelSpec>& channels,
                      std::vector<std::uint32_t>& activeChannels) {
    for (int physicalIndex = 0; physicalIndex < names.size(); ++physicalIndex) {
        const auto index = static_cast<std::uint32_t>(physicalIndex);
        channels.push_back({index, names[physicalIndex].isNotEmpty()
                                       ? names[physicalIndex]
                                       : fallbackPrefix + juce::String(physicalIndex + 1)});
        if (active[physicalIndex]) activeChannels.push_back(index);
    }
}

TransportStateSpec transportState(const TransportState state) noexcept {
    switch (state) {
        case TransportState::stopped:
            return TransportStateSpec::stopped;
        case TransportState::starting:
            return TransportStateSpec::starting;
        case TransportState::playing:
            return TransportStateSpec::playing;
        case TransportState::faulted:
            return TransportStateSpec::faulted;
    }
    return TransportStateSpec::faulted;
}

RecordingPhaseSpec recordingPhase(const RecordingPhase phase) noexcept {
    switch (phase) {
        case RecordingPhase::idle:
            return RecordingPhaseSpec::idle;
        case RecordingPhase::countingIn:
            return RecordingPhaseSpec::countingIn;
        case RecordingPhase::recording:
            return RecordingPhaseSpec::recording;
        case RecordingPhase::stopping:
            return RecordingPhaseSpec::stopping;
    }
    return RecordingPhaseSpec::idle;
}

}  // namespace

AudioStatusSpec AudioStatusBuilder::currentStatus(juce::AudioDeviceManager& manager,
                                                  const AudioRenderPipeline& pipeline,
                                                  const MidiMonitor& midi,
                                                  TimelineEngine& timeline) {
    AudioStatusSpec status;
    status.muteReasons = pipeline.getMuteReasons();
    if (pipeline.isDeviceFaulted()) {
        status.state = AudioStateSpec::faulted;
        status.message =
            "Audio device disconnected; output is muted and any captured take is preserved.";
    } else if (status.muteReasons != 0) {
        status.state = AudioStateSpec::muted;
        status.message = "Native audio is connected and muted.";
    } else {
        status.state = AudioStateSpec::ready;
        status.message = "Native audio is ready through the safety chain.";
    }

    const auto projectIdentity = timeline.activeProjectMeterIdentity();
    const auto transientMeters = pipeline.peekTransientMeters(projectIdentity.meterEpoch);
    status.inputPeak = transientMeters.inputPeak;
    status.outputPeak = transientMeters.outputPeak;
    status.invalidSamples = pipeline.getInvalidSampleCount();
    status.feedbackSuspected = pipeline.isFeedbackSuspected();
    status.previewing = pipeline.isPreviewing();
    status.instrumentPreviewing = pipeline.isInstrumentPreviewing();
    status.midiInputActive = midi.isActive();
    status.midiMessages = midi.getMessageCount();
    if (const auto lastNote = midi.getLastNote(); lastNote >= 0)
        status.lastMidiNote = static_cast<std::uint8_t>(lastNote);
    status.recording = pipeline.recordingStatus();

    auto& diagnostics = status.diagnostics;
    diagnostics.callbackCount = pipeline.getCallbackCount();
    diagnostics.averageCallbackDurationUs = pipeline.getAverageCallbackDurationUs();
    diagnostics.maximumCallbackDurationUs = pipeline.getMaximumCallbackDurationUs();
    diagnostics.callbackOverruns = pipeline.getCallbackOverruns();
    diagnostics.preLimiterPeak = transientMeters.preLimiterPeak;
    diagnostics.limiterGainReductionDb = transientMeters.limiterGainReductionDb;
    diagnostics.hardClipSamples = pipeline.getHardClipSamples();
    timeline.serviceDeferredCleanup();
    const auto timelineStatus = timeline.status();
    diagnostics.graphPublishCount = timelineStatus.graphPublishCount;
    if (const auto& graph = timelineStatus.graph) {
        status.timelineTick = graph->timelineTick;
        diagnostics.liveMidiDrops = graph->liveMidiDrops;
        diagnostics.trackCount = graph->trackCount;
        diagnostics.instrumentRuntimeCount = graph->instrumentRuntimeCount;
        diagnostics.pluginCount = graph->pluginCount;
        diagnostics.maximumLatencySamples = graph->maximumLatencySamples;
        diagnostics.graphRevision = graph->revision;
        diagnostics.instrumentFaults = graph->instrumentFaults;
    }
    const auto projectIdentityAfter = timeline.activeProjectMeterIdentity();
    if (projectIdentity.meterEpoch != projectIdentityAfter.meterEpoch ||
        projectIdentity.projectId != projectIdentityAfter.projectId) {
        status.inputPeak = 0.0;
        status.outputPeak = 0.0;
        diagnostics.preLimiterPeak = 0.0;
        diagnostics.limiterGainReductionDb = 0.0;
    }

    status.midiInputs = midiDevices(juce::MidiInput::getAvailableDevices());
    status.midiOutputs = midiDevices(juce::MidiOutput::getAvailableDevices());

    if (auto* device = manager.getCurrentAudioDevice()) {
        juce::AudioDeviceManager::AudioDeviceSetup setup;
        manager.getAudioDeviceSetup(setup);
        status.driver = device->getTypeName();
        status.inputDevice = setup.inputDeviceName;
        status.outputDevice = setup.outputDeviceName;
        status.inputChannel = static_cast<std::uint32_t>(pipeline.getInputChannel());
        describeChannels(device->getInputChannelNames(), device->getActiveInputChannels(), "Input ",
                         status.inputChannels, status.activeInputChannels);
        describeChannels(device->getOutputChannelNames(), device->getActiveOutputChannels(),
                         "Output ", status.outputChannels, status.activeOutputChannels);
        const auto sampleRate = device->getCurrentSampleRate();
        status.sampleRate = sampleRate;
        status.bufferSize = static_cast<std::uint32_t>(device->getCurrentBufferSizeSamples());
        const auto latencySamples =
            device->getInputLatencyInSamples() + device->getOutputLatencyInSamples();
        status.roundTripMs =
            sampleRate > 0.0 ? 1000.0 * static_cast<double>(latencySamples) / sampleRate : 0.0;
    }
    return status;
}

AudioMetersSpec AudioStatusBuilder::currentMeters(const AudioRenderPipeline& pipeline,
                                                  TimelineEngine& timeline) {
    const auto identityBefore = timeline.activeProjectMeterIdentity();
    const auto transientMeters = pipeline.consumeTransientMeters(identityBefore.meterEpoch);
    auto trackMeters = timeline.meterSnapshot();
    const auto identityAfter = timeline.activeProjectMeterIdentity();
    const auto identityStable = identityBefore.projectId == identityAfter.projectId &&
                                identityBefore.meterEpoch == identityAfter.meterEpoch;
    const auto stableMeters =
        identityStable ? transientMeters : AudioMetrics::TransientMeterSnapshot{};

    AudioMetersSpec meters;
    if (identityAfter.projectId.isNotEmpty()) meters.projectId = identityAfter.projectId;
    meters.inputPeak = stableMeters.inputPeak;
    meters.outputPeak = stableMeters.outputPeak;
    meters.outputPeakLeft = stableMeters.outputPeakLeft;
    meters.outputPeakRight = stableMeters.outputPeakRight;
    meters.invalidSamples = pipeline.getInvalidSampleCount();
    meters.preLimiterPeak = stableMeters.preLimiterPeak;
    meters.limiterGainReductionDb = stableMeters.limiterGainReductionDb;
    meters.hardClipSamples = pipeline.getHardClipSamples();
    meters.muteReasons = pipeline.getMuteReasons();
    meters.feedbackSuspected = pipeline.isFeedbackSuspected();
    meters.previewing = pipeline.isPreviewing();
    meters.instrumentPreviewing = pipeline.isInstrumentPreviewing();
    if (identityStable) meters.trackMeters = std::move(trackMeters);
    return meters;
}

TransportStatusSpec AudioStatusBuilder::currentTransport(const TimelineEngine& timeline) {
    const auto status = timeline.status();
    TransportStatusSpec transport;
    transport.state = transportState(status.transportState);
    transport.sequence = status.sequence;
    transport.recordingPhase = recordingPhase(status.recordingPhase);
    transport.recordingStartTick = status.recordingStartTick;
    transport.recordingPassOrdinal = status.recordingPassOrdinal;
    transport.clockGeneration = status.clockGeneration;
    transport.discontinuity = status.discontinuity;
    if (const auto& graph = status.graph) {
        transport.revision = graph->revision;
        transport.timelineTick = graph->timelineTick;
        transport.armedTrackIds = graph->armedTrackIds;
    }
    return transport;
}

}  // namespace riffra
