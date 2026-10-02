#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <memory>

#include "../AudioCommandDispatcher.h"
#include "audio/InstrumentPreviewSession.h"
#include "timeline/TimelineEngine.h"

namespace riffra {
namespace {

struct InstrumentPreviewResult final {
    bool success = false;
    juce::String error;
};

bool loadComparisonFile(juce::AudioFormatManager& formatManager, const double targetRate,
                        const juce::String& path, const std::uint64_t startFrame,
                        const std::uint64_t endFrame, juce::AudioBuffer<float>& target,
                        juce::String& loadError) {
    std::unique_ptr<juce::AudioFormatReader> reader(
        path.isEmpty() ? nullptr : formatManager.createReaderFor(juce::File(path)));
    if (reader == nullptr || reader->lengthInSamples <= 0 ||
        reader->lengthInSamples > std::numeric_limits<int>::max()) {
        loadError = "Take comparison source is unavailable.";
        return false;
    }
    if (endFrame <= startFrame || endFrame > static_cast<std::uint64_t>(reader->lengthInSamples) ||
        endFrame - startFrame > static_cast<std::uint64_t>(std::numeric_limits<int>::max())) {
        loadError = "Take comparison range is outside its source.";
        return false;
    }
    const auto sourceFrames = static_cast<int>(endFrame - startFrame);
    if (targetRate <= 0.0 || reader->sampleRate <= 0.0) {
        loadError = "Take comparison requires an active output sample rate.";
        return false;
    }
    juce::AudioBuffer<float> source(static_cast<int>(reader->numChannels), sourceFrames + 4);
    source.clear();
    if (!reader->read(&source, 0, sourceFrames, static_cast<juce::int64>(startFrame), true, true)) {
        loadError = "Take comparison source could not be read.";
        return false;
    }
    const auto targetFrames =
        std::max(1, static_cast<int>(std::llround(static_cast<double>(sourceFrames) * targetRate /
                                                  reader->sampleRate)));
    target.setSize(static_cast<int>(reader->numChannels), targetFrames);
    if (std::abs(reader->sampleRate - targetRate) <= 0.5) {
        for (int channel = 0; channel < target.getNumChannels(); ++channel)
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
}

}  // namespace

void AudioCommandDispatcher::handle(const StartTakeComparisonCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(responder,
                                "The Arrangement Graph is still loading a VST3. Take comparison "
                                "can be retried shortly."))
        return;
    const auto targetRate = context.pipeline.getSampleRate();
    juce::String comparisonError;
    if (!loadComparisonFile(context.formatManager, targetRate, command.rawPath,
                            command.rawStartFrame, command.rawEndFrame, context.comparisonRaw,
                            comparisonError) ||
        !loadComparisonFile(context.formatManager, targetRate, command.processedPath,
                            command.processedStartFrame, command.processedEndFrame,
                            context.comparisonProcessed, comparisonError) ||
        !context.pipeline.startPreview(context.comparisonRaw, 0,
                                       context.comparisonRaw.getNumSamples(), 1.0f, false,
                                       comparisonError, 1)) {
        responder.fail("takeComparison", comparisonError, "preview.takeComparison");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const SwitchTakeComparisonVariantCommand& command,
                                    CommandResponder responder) {
    juce::String comparisonError;
    const auto& buffer = command.variant == TakeComparisonVariantSpec::processed
                             ? context.comparisonProcessed
                             : context.comparisonRaw;
    if (!context.pipeline.switchPreviewBuffer(1, buffer, comparisonError)) {
        responder.fail("takeComparison", comparisonError, "preview.takeComparison");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopTakeComparisonCommand&, CommandResponder responder) {
    juce::String error;
    if (!context.pipeline.stopPreviewForKey(1, &error)) {
        responder.fail("takeComparison", error, "preview.takeComparison.stop");
        return;
    }
    context.comparisonRaw.setSize(0, 0);
    context.comparisonProcessed.setSize(0, 0);
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const PreviewSampleCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still loading a VST3. Preview can be retried shortly."))
        return;
    std::unique_ptr<juce::AudioFormatReader> reader(
        command.path.isEmpty() ? nullptr
                               : context.formatManager.createReaderFor(juce::File(command.path)));
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
            const auto toSample = [&reader](const std::uint64_t milliseconds) {
                return static_cast<int>(
                    std::llround(static_cast<double>(milliseconds) * reader->sampleRate / 1000.0));
            };
            const auto start = juce::jlimit(0, static_cast<int>(length), toSample(command.startMs));
            const auto end =
                command.endMs.value_or(0) > 0
                    ? juce::jlimit(start + 1, static_cast<int>(length), toSample(*command.endMs))
                    : static_cast<int>(length);
            if (!context.pipeline.startPreview(buffer, start, end, static_cast<float>(command.gain),
                                               command.loop, previewError, -1) &&
                previewError.isEmpty())
                previewError = "Preview range is invalid.";
        }
    }
    if (previewError.isNotEmpty()) {
        responder.fail("preview", previewError, "preview.sample");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const PreviewInstrumentCommand& command,
                                    CommandResponder responder) {
    if (rejectWhileTimelineBusy(
            responder,
            "The Arrangement Graph is still loading a VST3. Preview can be retried shortly."))
        return;
    if (command.definitionJson.isEmpty() || command.definitionBaseDir.isEmpty()) {
        responder.fail("preview", "Instrument preview definition is unavailable.",
                       "preview.instrument");
        return;
    }

    constexpr auto preparationTimeout = std::chrono::seconds(45);
    const auto result = std::make_shared<InstrumentPreviewResult>();
    const auto completion = std::make_shared<std::promise<void>>();
    auto completed = completion->get_future();
    const auto submitted = context.runtimeLifecycle.submit(
        [this, definitionJson = command.definitionJson,
         definitionBaseDir = command.definitionBaseDir, spec = command.preview, result,
         completion]() mutable {
            try {
                result->success = context.pipeline.startInstrumentPreview(
                    definitionJson, definitionBaseDir, std::move(spec), result->error);
                if (!result->success && result->error.isEmpty())
                    result->error = "Instrument preview could not be prepared.";
            } catch (...) {
                result->error = "Instrument preview could not be prepared.";
            }
            completion->set_value();
        },
        preparationTimeout);
    if (!submitted || completed.wait_for(preparationTimeout) != std::future_status::ready) {
        responder.fail("preview", "Instrument preview preparation timed out.",
                       "preview.instrument");
        return;
    }
    if (!result->success) {
        responder.fail("preview", result->error, "preview.instrument");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopPreviewCommand&, CommandResponder responder) {
    juce::String error;
    if (!context.pipeline.stopPreview(&error)) {
        responder.fail("preview",
                       error.isNotEmpty() ? error : "The realtime preview command queue is full.",
                       "preview.stop");
        return;
    }
    responder.respond(currentStatus());
}

void AudioCommandDispatcher::handle(const StopInstrumentPreviewCommand&,
                                    CommandResponder responder) {
    juce::String error;
    if (!context.pipeline.stopInstrumentPreview(&error)) {
        responder.fail("preview", error, "preview.instrument.stop");
        return;
    }
    responder.respond(currentStatus());
}

}  // namespace riffra
