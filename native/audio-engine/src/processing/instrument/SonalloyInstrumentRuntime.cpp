#include "SonalloyInstrumentRuntime.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

namespace riffra {
namespace {

SonalloyStringView stringView(const juce::String& value) noexcept {
    return {value.toRawUTF8(), static_cast<std::size_t>(value.getNumBytesAsUTF8())};
}

const char* diagnosticSeverityName(const std::uint32_t severity) noexcept {
    switch (severity) {
        case 0:
            return "error";
        case 1:
            return "warning";
        case 2:
            return "info";
        default:
            return "unknown";
    }
}

juce::String diagnosticText(const SonalloyStringView value) {
    if (value.data == nullptr || value.length == 0) return {};
    constexpr int kMaximumDiagnosticText = 512;
    return juce::String::fromUTF8(value.data, static_cast<int>(std::min<std::size_t>(
                                                  value.length, kMaximumDiagnosticText)))
        .trim();
}

bool declaresUnsupportedExternalAudio(const juce::String& definitionJson) {
    const auto definition = juce::JSON::parse(definitionJson);
    if (!definition.isObject()) return false;

    const auto externalAudio = definition.getProperty("external_audio", {});
    if (!externalAudio.isObject()) return false;
    const auto channels = externalAudio.getProperty("channels", {}).toString();
    return channels == "mono" || channels == "stereo";
}

}  // namespace

void SonalloyInstrumentRuntime::CompiledDeleter::operator()(
    SonalloyCompiledInstrument* const value) const noexcept {
    if (value != nullptr) sonalloy_compiled_destroy(value);
}

void SonalloyInstrumentRuntime::DiagnosticsDeleter::operator()(
    SonalloyDiagnostics* const value) const noexcept {
    if (value != nullptr) sonalloy_diagnostics_destroy(value);
}

void SonalloyInstrumentRuntime::RuntimeDeleter::operator()(
    SonalloyRuntime* const value) const noexcept {
    if (value != nullptr) sonalloy_runtime_destroy(value);
}

SonalloyInstrumentRuntime::SonalloyInstrumentRuntime(CompiledPtr compiled, RuntimePtr runtime,
                                                     const int blockSize,
                                                     const int latencySamples) noexcept
    : compiled(std::move(compiled)),
      runtime(std::move(runtime)),
      blockSize(blockSize),
      reportedLatencySamples(latencySamples) {}

std::unique_ptr<SonalloyInstrumentRuntime> SonalloyInstrumentRuntime::create(
    const juce::String& definitionJson, const juce::String& definitionBaseDir,
    const double sampleRate, const int blockSize, juce::String& error) {
    if (definitionJson.isEmpty() || definitionBaseDir.isEmpty() || !std::isfinite(sampleRate) ||
        sampleRate <= 0.0 || blockSize <= 0) {
        error = "Built-in instrument definition or process specification is invalid.";
        return nullptr;
    }
    if (declaresUnsupportedExternalAudio(definitionJson)) {
        error =
            "This built-in instrument requires an audio input route that Riffra does not support "
            "yet.";
        return nullptr;
    }

    const SonalloyProcessSpec spec{
        sampleRate,
        static_cast<std::uint32_t>(blockSize),
        0,
        2,
    };
    SonalloyCompiledInstrument* compiledRaw = nullptr;
    SonalloyDiagnostics* diagnosticsRaw = nullptr;
    const auto result =
        sonalloy_compile_json(stringView(definitionJson), stringView(definitionBaseDir), spec,
                              &compiledRaw, &diagnosticsRaw);
    std::unique_ptr<SonalloyDiagnostics, DiagnosticsDeleter> diagnostics(diagnosticsRaw);
    CompiledPtr compiled(compiledRaw);
    if (result != SONALLOY_OK || compiled == nullptr) {
        error = diagnosticsSummary(diagnostics.get(), result);
        return nullptr;
    }
    const auto latency = static_cast<int>(
        std::min<std::uint32_t>(sonalloy_compiled_reported_latency_frames(compiled.get()),
                                static_cast<std::uint32_t>(std::numeric_limits<int>::max())));
    SonalloyRuntime* runtimeRaw = nullptr;
    auto runtimeResult = sonalloy_runtime_create(compiled.get(), &runtimeRaw);
    RuntimePtr runtime(runtimeRaw);
    if (runtimeResult != SONALLOY_OK || runtime == nullptr) {
        error = "Built-in instrument runtime creation failed: " + resultName(runtimeResult) + ".";
        return nullptr;
    }
    runtimeResult = sonalloy_runtime_prepare(runtime.get(), spec);
    if (runtimeResult != SONALLOY_OK) {
        error =
            "Built-in instrument runtime preparation failed: " + resultName(runtimeResult) + ".";
        return nullptr;
    }
    runtimeResult = sonalloy_runtime_activate(runtime.get());
    if (runtimeResult != SONALLOY_OK) {
        error = "Built-in instrument runtime activation failed: " + resultName(runtimeResult) + ".";
        return nullptr;
    }
    return std::unique_ptr<SonalloyInstrumentRuntime>(
        new SonalloyInstrumentRuntime(std::move(compiled), std::move(runtime), blockSize, latency));
}

juce::String SonalloyInstrumentRuntime::diagnosticsSummary(
    const SonalloyDiagnostics* const diagnostics, const SonalloyResult result) {
    juce::String summary =
        "Built-in instrument definition compilation failed: " + resultName(result);
    if (diagnostics == nullptr) return summary + ".";
    const auto count = std::min<std::uint32_t>(sonalloy_diagnostics_count(diagnostics), 8);
    for (std::uint32_t index = 0; index < count; ++index) {
        SonalloyDiagnosticView diagnostic{};
        if (sonalloy_diagnostics_get(diagnostics, index, &diagnostic) != SONALLOY_OK) continue;
        const auto path = diagnosticText(diagnostic.path);
        const auto message = diagnosticText(diagnostic.message);
        const auto detail = diagnosticText(diagnostic.detail);
        summary += " [" + juce::String(static_cast<int>(diagnostic.code)) + "/" +
                   diagnosticSeverityName(diagnostic.severity) + "]";
        if (path.isNotEmpty()) summary += " " + path + ":";
        if (message.isNotEmpty()) summary += " " + message;
        if (detail.isNotEmpty()) summary += " (" + detail + ")";
    }
    return summary + ".";
}

juce::String SonalloyInstrumentRuntime::resultName(const SonalloyResult result) {
    switch (result) {
        case SONALLOY_OK:
            return "ok";
        case SONALLOY_INVALID_ARGUMENT:
            return "invalid argument";
        case SONALLOY_INVALID_STATE:
            return "invalid state";
        case SONALLOY_COMPILE_FAILED:
            return "compile failed";
        case SONALLOY_PREPARE_FAILED:
            return "prepare failed";
        case SONALLOY_PROCESS_FAILED:
            return "process failed";
        case SONALLOY_UPDATE_INCOMPATIBLE:
            return "incompatible update";
        case SONALLOY_UPDATE_CAPACITY_EXCEEDED:
            return "update capacity exceeded";
        case SONALLOY_TRANSITION_BUSY:
            return "transition busy";
        case SONALLOY_INTERNAL_PANIC:
            return "internal panic";
        default:
            return "unknown failure";
    }
}

bool SonalloyInstrumentRuntime::isLoaded() const noexcept {
    return compiled != nullptr && runtime != nullptr;
}

void SonalloyInstrumentRuntime::clearOutputs(float* const* outputChannels,
                                             const int outputChannelCount,
                                             const int numSamples) noexcept {
    if (outputChannels == nullptr || numSamples <= 0) return;
    for (int channel = 0; channel < outputChannelCount; ++channel)
        if (outputChannels[channel] != nullptr)
            juce::FloatVectorOperations::clear(outputChannels[channel], numSamples);
}

void SonalloyInstrumentRuntime::clearActiveNotes() noexcept {
    for (auto& note : activeNotes) note.active = false;
}

bool SonalloyInstrumentRuntime::resetRuntimeInCallback() noexcept {
    clearActiveNotes();
    absoluteFrame = 0;
    nextNoteId = 1;
    if (runtime == nullptr) return false;
    const auto result = sonalloy_runtime_reset(runtime.get());
    if (result != SONALLOY_OK) {
        lastFaultCode.store(static_cast<std::uint32_t>(result), std::memory_order_release);
        return false;
    }
    return true;
}

void SonalloyInstrumentRuntime::failBlock(const std::uint32_t code, float* const* outputChannels,
                                          const int outputChannelCount,
                                          const int numSamples) noexcept {
    lastFaultCode.store(code, std::memory_order_release);
    midiGeneration.fetch_add(1, std::memory_order_acq_rel);
    (void)resetRuntimeInCallback();
    clearOutputs(outputChannels, outputChannelCount, numSamples);
}

int SonalloyInstrumentRuntime::eventPriority(const SonalloyEvent& event) noexcept {
    switch (event.event_type) {
        case SONALLOY_EVENT_SUSTAIN:
            return 0;
        case SONALLOY_EVENT_NOTE_OFF:
            return 1;
        case SONALLOY_EVENT_PITCH_BEND:
        case SONALLOY_EVENT_MOD_WHEEL:
        case SONALLOY_EVENT_AFTERTOUCH:
            return 2;
        case SONALLOY_EVENT_NOTE_ON:
            return 3;
        default:
            return 4;
    }
}

bool SonalloyInstrumentRuntime::appendEvent(SonalloyEvent event,
                                            std::uint32_t& eventCount) noexcept {
    if (eventCount >= events.size()) return false;
    event.sample_offset = std::min<std::uint32_t>(
        event.sample_offset, static_cast<std::uint32_t>(std::max(0, blockSize - 1)));
    auto insertion = eventCount;
    const auto priority = eventPriority(event);
    while (insertion > 0) {
        const auto& previous = events[insertion - 1];
        if (previous.sample_offset < event.sample_offset ||
            (previous.sample_offset == event.sample_offset && eventPriority(previous) <= priority))
            break;
        events[insertion] = previous;
        --insertion;
    }
    events[insertion] = event;
    ++eventCount;
    return true;
}

SonalloyInstrumentRuntime::ActiveNote* SonalloyInstrumentRuntime::allocateActiveNote() noexcept {
    for (auto& note : activeNotes)
        if (!note.active) return &note;
    return nullptr;
}

SonalloyInstrumentRuntime::ActiveNote* SonalloyInstrumentRuntime::findLatestActiveNote(
    const std::uint8_t channel, const std::uint8_t noteNumber,
    const std::uint32_t sampleOffset) noexcept {
    ActiveNote* latest = nullptr;
    for (auto& note : activeNotes) {
        if (!note.active || note.channel != channel || note.noteNumber != noteNumber) continue;
        // Events at the same sample offset are delivered with Note Off before
        // Note On. A Note On already collected at this offset therefore cannot
        // be the voice released by this Note Off, even when input MIDI order
        // placed it first.
        if (note.startedBlock == processBlockSerial && note.startedSampleOffset >= sampleOffset)
            continue;
        if (latest == nullptr || note.startedOrder > latest->startedOrder) latest = &note;
    }
    return latest;
}

bool SonalloyInstrumentRuntime::appendNoteOn(const std::uint8_t channel,
                                             const std::uint8_t noteNumber,
                                             const std::uint8_t velocity,
                                             const std::uint32_t sampleOffset,
                                             std::uint32_t& eventCount) noexcept {
    auto* active = allocateActiveNote();
    if (active == nullptr) return false;
    if (nextNoteId == 0) nextNoteId = 1;
    const auto noteId = nextNoteId++;
    active->active = true;
    active->channel = channel;
    active->noteNumber = noteNumber;
    active->noteId = noteId;
    active->startedOrder = noteId;
    active->startedBlock = processBlockSerial;
    active->startedSampleOffset = sampleOffset;
    SonalloyEvent event{};
    event.sample_offset = sampleOffset;
    event.event_type = SONALLOY_EVENT_NOTE_ON;
    event.note_id = noteId;
    event.note_number = active->noteNumber;
    event.velocity = velocity;
    return appendEvent(event, eventCount);
}

bool SonalloyInstrumentRuntime::appendNoteOff(const std::uint8_t channel,
                                              const std::uint8_t noteNumber,
                                              const std::uint32_t sampleOffset,
                                              std::uint32_t& eventCount) noexcept {
    auto* active = findLatestActiveNote(channel, noteNumber, sampleOffset);
    if (active == nullptr) return true;
    SonalloyEvent event{};
    event.sample_offset = sampleOffset;
    event.event_type = SONALLOY_EVENT_NOTE_OFF;
    event.note_id = active->noteId;
    active->active = false;
    return appendEvent(event, eventCount);
}

bool SonalloyInstrumentRuntime::appendMidiBytes(const std::uint8_t* const data,
                                                const std::size_t size,
                                                const std::uint32_t sampleOffset,
                                                std::uint32_t& eventCount) noexcept {
    if (data == nullptr || size == 0 || (data[0] & 0x80u) == 0) return true;
    const auto status = static_cast<std::uint8_t>(data[0] & 0xf0u);
    const auto channel = static_cast<std::uint8_t>((data[0] & 0x0fu) + 1u);
    if ((status == 0x80u || status == 0x90u || status == 0xb0u || status == 0xe0u) && size < 3)
        return true;
    if (status == 0xd0u && size < 2) return true;

    if (status == 0x80u || status == 0x90u) {
        const auto noteNumber = static_cast<std::uint8_t>(data[1] & 0x7fu);
        if (status == 0x80u) return appendNoteOff(channel, noteNumber, sampleOffset, eventCount);
        const auto velocity = static_cast<std::uint8_t>(data[2] & 0x7fu);
        return velocity == 0
                   ? appendNoteOff(channel, noteNumber, sampleOffset, eventCount)
                   : appendNoteOn(channel, noteNumber, velocity, sampleOffset, eventCount);
    }

    SonalloyEvent event{};
    event.sample_offset = sampleOffset;
    if (status == 0xb0u) {
        const auto controller = static_cast<std::uint8_t>(data[1] & 0x7fu);
        if (controller == 64) {
            event.event_type = SONALLOY_EVENT_SUSTAIN;
            event.bool_value = (data[2] & 0x7fu) >= 64 ? 1 : 0;
        } else if (controller == 1) {
            event.event_type = SONALLOY_EVENT_MOD_WHEEL;
            event.value = static_cast<float>(data[2] & 0x7fu) / 127.0f;
        } else {
            return true;
        }
    } else if (status == 0xe0u) {
        event.event_type = SONALLOY_EVENT_PITCH_BEND;
        const auto value = static_cast<int>((data[1] & 0x7fu) | ((data[2] & 0x7fu) << 7));
        event.value = static_cast<float>(value - 8192) / 8192.0f;
    } else if (status == 0xd0u) {
        event.event_type = SONALLOY_EVENT_AFTERTOUCH;
        event.value = static_cast<float>(data[1] & 0x7fu) / 127.0f;
    } else {
        return true;
    }
    return appendEvent(event, eventCount);
}

bool SonalloyInstrumentRuntime::appendPendingMidi(const PendingMidi& pending,
                                                  std::uint32_t& eventCount) noexcept {
    return appendMidiBytes(pending.bytes.data(), pending.size, 0, eventCount);
}

bool SonalloyInstrumentRuntime::enqueueMidi(const juce::MidiMessage& message) noexcept {
    if (!isLoaded()) return false;
    const auto size = message.getRawDataSize();
    if (size <= 0 || static_cast<std::size_t>(size) > PendingMidi::kMaximumMessageBytes) {
        droppedMidi.fetch_add(1, std::memory_order_relaxed);
        return false;
    }
    PendingMidi pending;
    pending.generation = midiGeneration.load(std::memory_order_acquire);
    pending.size = static_cast<std::uint16_t>(size);
    std::copy_n(message.getRawData(), size, pending.bytes.begin());
    return pendingMidi.tryPush(pending);
}

void SonalloyInstrumentRuntime::allNotesOff() noexcept {
    midiGeneration.fetch_add(1, std::memory_order_acq_rel);
    resetPending.store(true, std::memory_order_release);
}

void SonalloyInstrumentRuntime::resetForTransportDiscontinuity() noexcept {
    midiGeneration.fetch_add(1, std::memory_order_acq_rel);
    resetPending.store(true, std::memory_order_release);
}

int SonalloyInstrumentRuntime::latencySamples() const noexcept { return reportedLatencySamples; }

int SonalloyInstrumentRuntime::tailSamples() const noexcept { return 0; }

void SonalloyInstrumentRuntime::setBypassed(const bool shouldBypass) noexcept {
    bypassed.store(shouldBypass, std::memory_order_release);
}

void SonalloyInstrumentRuntime::process(float* const* outputChannels, const int outputChannelCount,
                                        const int numSamples, const juce::MidiBuffer* const midi,
                                        const InstrumentProcessContext& context) noexcept {
    clearOutputs(outputChannels, outputChannelCount, numSamples);
    if (!isLoaded() || numSamples <= 0) return;
    if (numSamples > blockSize || outputChannelCount != 2 || outputChannels == nullptr ||
        outputChannels[0] == nullptr || outputChannels[1] == nullptr) {
        failBlock(SONALLOY_INVALID_ARGUMENT, outputChannels, outputChannelCount, numSamples);
        return;
    }
    if (resetPending.exchange(false, std::memory_order_acq_rel) && !resetRuntimeInCallback()) {
        failBlock(SONALLOY_INVALID_STATE, outputChannels, outputChannelCount, numSamples);
        return;
    }

    ++processBlockSerial;
    std::uint32_t eventCount = 0;
    PendingMidi pending;
    const auto midiGenerationForBlock = midiGeneration.load(std::memory_order_acquire);
    while (pendingMidi.tryPop(pending)) {
        if (pending.generation < midiGenerationForBlock) continue;
        if (!appendPendingMidi(pending, eventCount)) {
            failBlock(SONALLOY_INTERNAL_PANIC, outputChannels, outputChannelCount, numSamples);
            return;
        }
    }
    if (midi != nullptr) {
        for (const auto metadata : *midi) {
            if (metadata.data == nullptr || metadata.numBytes <= 0 || metadata.samplePosition < 0 ||
                metadata.samplePosition >= numSamples ||
                !appendMidiBytes(metadata.data, static_cast<std::size_t>(metadata.numBytes),
                                 static_cast<std::uint32_t>(metadata.samplePosition), eventCount)) {
                failBlock(SONALLOY_INTERNAL_PANIC, outputChannels, outputChannelCount, numSamples);
                return;
            }
        }
    }

    SonalloyProcessContext processContext{
        absoluteFrame,
        context.tempoBpm,
        context.beatPosition,
        context.barPosition,
        context.timeSignatureNumerator,
        context.timeSignatureDenominator,
        context.playing ? SONALLOY_TRANSPORT_PLAYING : SONALLOY_TRANSPORT_STOPPED,
    };
    const auto result =
        sonalloy_runtime_process(runtime.get(), &processContext, events.data(), eventCount, nullptr,
                                 0, outputChannels, 2, static_cast<std::uint32_t>(numSamples));
    if (result != SONALLOY_OK) {
        failBlock(static_cast<std::uint32_t>(result), outputChannels, outputChannelCount,
                  numSamples);
        return;
    }
    absoluteFrame += static_cast<std::uint64_t>(numSamples);
    if (bypassed.load(std::memory_order_acquire))
        clearOutputs(outputChannels, outputChannelCount, numSamples);
}

std::uint32_t SonalloyInstrumentRuntime::faultCode() const noexcept {
    return lastFaultCode.load(std::memory_order_acquire);
}

std::uint64_t SonalloyInstrumentRuntime::droppedMidiEvents() const noexcept {
    return droppedMidi.load(std::memory_order_acquire) + pendingMidi.droppedPushes();
}

}  // namespace riffra
