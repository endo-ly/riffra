#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <memory>

#include "../AudioCommandDispatcher.h"
#include "audio/InstrumentPreviewContract.h"
#include "audio/InstrumentPreviewSession.h"
#include "midi/MidiInputService.h"
#include "protocol/AudioProtocol.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

bool readPreviewInteger(const juce::var& object, const char* const propertyName,
                        const std::uint64_t maximum, std::uint64_t& value) {
    const auto property = object.getProperty(propertyName, {});
    if (property.isInt()) {
        const auto candidate = static_cast<int>(property);
        if (candidate < 0) return false;
        value = static_cast<std::uint64_t>(candidate);
        return value <= maximum;
    }
    if (property.isInt64()) {
        const auto candidate = static_cast<juce::int64>(property);
        if (candidate < 0) return false;
        value = static_cast<std::uint64_t>(candidate);
        return value <= maximum;
    }
    if (!property.isDouble()) return false;
    const auto candidate = static_cast<long double>(static_cast<double>(property));
    if (!std::isfinite(static_cast<double>(candidate)) || candidate < 0.0L ||
        std::floor(candidate) != candidate || candidate > static_cast<long double>(maximum) ||
        (maximum == std::numeric_limits<std::uint64_t>::max() &&
         candidate >= static_cast<long double>(std::numeric_limits<std::uint64_t>::max())))
        return false;
    value = static_cast<std::uint64_t>(candidate);
    return true;
}

bool readPreviewNumber(const juce::var& object, const char* const propertyName, double& value) {
    const auto property = object.getProperty(propertyName, {});
    if (!property.isInt() && !property.isInt64() && !property.isDouble()) return false;
    value = static_cast<double>(property);
    return std::isfinite(value);
}

bool parsePreviewSpec(const juce::var& value, InstrumentPreviewSpec& spec, juce::String& error) {
    if (!value.isObject()) {
        error = "Built-in instrument preview must contain an object preview definition.";
        return false;
    }

    double tempoBpm = 0.0;
    std::uint64_t ticksPerBeat = 0;
    std::uint64_t lengthTicks = 0;
    if (!readPreviewNumber(value, "tempoBpm", tempoBpm) ||
        !readPreviewInteger(value, "ticksPerBeat", instrument_preview::kMaximumTicksPerBeat,
                            ticksPerBeat) ||
        !readPreviewInteger(value, "lengthTicks", std::numeric_limits<std::uint64_t>::max(),
                            lengthTicks) ||
        !instrument_preview::isValidTempo(tempoBpm) ||
        !instrument_preview::isValidTicksPerBeat(static_cast<std::uint16_t>(ticksPerBeat)) ||
        !instrument_preview::isWithinDurationLimit(
            tempoBpm, static_cast<std::uint16_t>(ticksPerBeat), lengthTicks)) {
        error = "Built-in instrument preview has an invalid tempo or length.";
        return false;
    }

    const auto timeSignature = value.getProperty("timeSignature", {});
    std::uint64_t numerator = 0;
    std::uint64_t denominator = 0;
    if (!timeSignature.isObject() ||
        !readPreviewInteger(timeSignature, "numerator", std::numeric_limits<std::uint8_t>::max(),
                            numerator) ||
        !instrument_preview::isValidNumerator(static_cast<std::uint8_t>(numerator)) ||
        !readPreviewInteger(timeSignature, "denominator", std::numeric_limits<std::uint8_t>::max(),
                            denominator) ||
        !instrument_preview::isValidDenominator(static_cast<std::uint8_t>(denominator))) {
        error = "Built-in instrument preview has an invalid time signature.";
        return false;
    }

    const auto notes = value.getProperty("notes", {});
    if (!notes.isArray() || notes.size() < instrument_preview::kMinimumNoteCount ||
        notes.size() > instrument_preview::kMaximumNoteCount) {
        error = "Built-in instrument preview notes are invalid.";
        return false;
    }

    spec.tempoBpm = tempoBpm;
    spec.ticksPerBeat = static_cast<std::uint16_t>(ticksPerBeat);
    spec.timeSignature = InstrumentPreviewTimeSignature{static_cast<std::uint8_t>(numerator),
                                                        static_cast<std::uint8_t>(denominator)};
    spec.lengthTicks = lengthTicks;
    spec.notes.clear();
    spec.notes.reserve(static_cast<std::size_t>(notes.size()));
    std::uint64_t previousTick = 0;
    bool hasPreviousTick = false;
    for (const auto& noteValue : *notes.getArray()) {
        if (!noteValue.isObject()) {
            error = "Built-in instrument preview contains an invalid note.";
            return false;
        }
        std::uint64_t tick = 0;
        std::uint64_t durationTicks = 0;
        std::uint64_t note = 0;
        std::uint64_t velocity = 0;
        if (!readPreviewInteger(noteValue, "tick", std::numeric_limits<std::uint64_t>::max(),
                                tick) ||
            !readPreviewInteger(noteValue, "durationTicks",
                                std::numeric_limits<std::uint64_t>::max(), durationTicks) ||
            !readPreviewInteger(noteValue, "note", 127, note) ||
            !readPreviewInteger(noteValue, "velocity", 127, velocity) || durationTicks == 0 ||
            tick >= lengthTicks || durationTicks > lengthTicks ||
            tick > lengthTicks - durationTicks || velocity == 0 ||
            (hasPreviousTick && tick < previousTick)) {
            error = "Built-in instrument preview contains an invalid note.";
            return false;
        }
        previousTick = tick;
        hasPreviousTick = true;
        spec.notes.push_back(InstrumentPreviewNote{tick, durationTicks,
                                                   static_cast<std::uint8_t>(note),
                                                   static_cast<std::uint8_t>(velocity)});
    }
    return true;
}

struct BuiltInPreviewResult final {
    bool success = false;
    juce::String error;
};

}  // namespace

CommandResult AudioCommandDispatcher::dispatchPreview(const juce::var& command) {
    const auto type = command.getProperty("type", {}).toString();
    if (type == "startTakeComparison") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Take "
                                "comparison can be retried shortly."));
            return {};
        }
        const auto loadComparisonFile = [&](const juce::String& path, const juce::int64 startFrame,
                                            const juce::int64 endFrame,
                                            juce::AudioBuffer<float>& target,
                                            juce::String& loadError) {
            std::unique_ptr<juce::AudioFormatReader> reader(
                path.isEmpty() ? nullptr : context.formatManager.createReaderFor(juce::File(path)));
            if (reader == nullptr || reader->lengthInSamples <= 0 ||
                reader->lengthInSamples > std::numeric_limits<int>::max()) {
                loadError = "Take comparison source is unavailable.";
                return false;
            }
            if (startFrame < 0 || endFrame <= startFrame || endFrame > reader->lengthInSamples ||
                endFrame - startFrame > std::numeric_limits<int>::max()) {
                loadError = "Take comparison range is outside its source.";
                return false;
            }
            const auto sourceFrames = static_cast<int>(endFrame - startFrame);
            const auto targetRate = context.pipeline.getSampleRate();
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
            const auto targetFrames =
                std::max(1, static_cast<int>(std::llround(static_cast<double>(sourceFrames) *
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
        if (!loadComparisonFile(command.getProperty("rawPath", {}).toString(),
                                static_cast<juce::int64>(command.getProperty("rawStartFrame", 0)),
                                static_cast<juce::int64>(command.getProperty("rawEndFrame", 0)),
                                context.comparisonRaw, comparisonError) ||
            !loadComparisonFile(
                command.getProperty("processedPath", {}).toString(),
                static_cast<juce::int64>(command.getProperty("processedStartFrame", 0)),
                static_cast<juce::int64>(command.getProperty("processedEndFrame", 0)),
                context.comparisonProcessed, comparisonError) ||
            !context.pipeline.startPreview(context.comparisonRaw, 0,
                                           context.comparisonRaw.getNumSamples(), 1.0f, false,
                                           comparisonError, 1)) {
            writeJson(makeError("takeComparison", comparisonError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "switchTakeComparisonVariant") {
        const auto variant = command.getProperty("variant", {}).toString();
        juce::String comparisonError;
        const auto& buffer =
            variant == "processed" ? context.comparisonProcessed : context.comparisonRaw;
        if (!context.pipeline.switchPreviewBuffer(1, buffer, comparisonError)) {
            writeJson(makeError("takeComparison", comparisonError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "stopTakeComparison") {
        context.pipeline.stopPreviewForKey(1);
        context.comparisonRaw.setSize(0, 0);
        context.comparisonProcessed.setSize(0, 0);
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "previewSample") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Preview "
                                "can be retried shortly."));
            return {};
        }
        const auto path = command.getProperty("path", {}).toString();
        std::unique_ptr<juce::AudioFormatReader> reader(
            path.isEmpty() ? nullptr : context.formatManager.createReaderFor(juce::File(path)));
        juce::String previewError;
        const auto sampleRate = context.pipeline.getSampleRate();
        if (reader == nullptr) {
            previewError = "Preview source could not be opened as an audio file.";
        } else if (sampleRate <= 0.0 || std::abs(reader->sampleRate - sampleRate) > 0.5) {
            previewError = "Preview source sample rate does not match the active audio device.";
        } else {
            const auto length = std::min<juce::int64>(
                reader->lengthInSamples, static_cast<juce::int64>(std::numeric_limits<int>::max()));
            juce::AudioBuffer<float> buffer(reader->numChannels, static_cast<int>(length));
            if (length <= 0 || !reader->read(&buffer, 0, static_cast<int>(length), 0, true, true)) {
                previewError = "Preview source contains no readable audio samples.";
            } else {
                const auto startMs = static_cast<double>(command.getProperty("startMs", 0.0));
                const auto endMs = static_cast<double>(command.getProperty("endMs", -1.0));
                const auto start = juce::jlimit(
                    0, static_cast<int>(length),
                    static_cast<int>(std::llround(startMs * reader->sampleRate / 1000.0)));
                const auto end =
                    endMs <= 0.0
                        ? static_cast<int>(length)
                        : juce::jlimit(
                              start + 1, static_cast<int>(length),
                              static_cast<int>(std::llround(endMs * reader->sampleRate / 1000.0)));
                if (!context.pipeline.startPreview(
                        buffer, start, end,
                        static_cast<float>(static_cast<double>(command.getProperty("gain", 1.0))),
                        static_cast<bool>(command.getProperty("loop", false)), previewError, -1))
                    previewError =
                        previewError.isEmpty() ? "Preview range is invalid." : previewError;
            }
        }
        if (previewError.isNotEmpty()) {
            writeJson(makeError("preview", previewError));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "previewBuiltInInstrument") {
        if (context.timelineOperationRunning.load(std::memory_order_acquire)) {
            writeJson(makeError("timelineBusy",
                                "The Arrangement Graph is still loading a VST3. Preview "
                                "can be retried shortly."));
            return {};
        }
        const auto definitionJson = command.getProperty("definitionJson", {}).toString();
        const auto definitionBaseDir = command.getProperty("definitionBaseDir", {}).toString();
        InstrumentPreviewSpec spec;
        juce::String previewError;
        if (definitionJson.isEmpty() || definitionBaseDir.isEmpty() ||
            !parsePreviewSpec(command.getProperty("preview", {}), spec, previewError)) {
            if (previewError.isEmpty())
                previewError = "Built-in instrument preview definition is unavailable.";
            writeJson(makeError("preview", previewError));
            return {};
        }

        constexpr auto preparationTimeout = std::chrono::seconds(45);
        const auto result = std::make_shared<BuiltInPreviewResult>();
        const auto completion = std::make_shared<std::promise<void>>();
        auto completed = completion->get_future();
        const auto submitted = context.runtimeLifecycle.submit(
            [this, definitionJson, definitionBaseDir, spec = std::move(spec), result,
             completion]() mutable {
                try {
                    result->success = context.pipeline.startBuiltInPreview(
                        definitionJson, definitionBaseDir, std::move(spec), result->error);
                    if (!result->success && result->error.isEmpty())
                        result->error = "Built-in instrument preview could not be prepared.";
                } catch (...) {
                    result->error = "Built-in instrument preview could not be prepared.";
                }
                completion->set_value();
            },
            preparationTimeout);
        if (!submitted || completed.wait_for(preparationTimeout) != std::future_status::ready) {
            writeJson(makeError("preview", "Built-in instrument preview preparation timed out."));
            return {};
        }
        if (!result->success) {
            writeJson(makeError("preview", result->error));
            return {};
        }
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "stopPreview") {
        context.pipeline.stopPreview();
        context.pipeline.allNotesOff();
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }

    if (type == "stopBuiltInInstrumentPreview") {
        context.pipeline.stopBuiltInPreview();
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    return {};
}

}  // namespace riffra
