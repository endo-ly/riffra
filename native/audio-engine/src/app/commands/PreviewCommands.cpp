#include "../AudioCommandDispatcher.h"
#include "AudioProtocol.h"
#include "MidiInputService.h"
#include "TimelineEngine.h"

namespace riffra {

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

    if (type == "stopPreview") {
        context.pipeline.stopPreview();
        context.pipeline.allNotesOff();
        writeJson(AudioStatusBuilder::currentStatus(context.deviceController.manager(),
                                                    context.pipeline, &context.midiInputs.monitor(),
                                                    {}, &context.timelineEngine));
        return {};
    }
    return {};
}

}  // namespace riffra
