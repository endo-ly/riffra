#pragma once

#include "pipeline/AudioRenderPipeline.h"

namespace riffra {

/// Prepares the pipeline for a fixed device format without a real audio device.
class AudioRenderPipelineTestPeer final {
public:
    static void prepare(AudioRenderPipeline& pipeline, const double sampleRate,
                        const int blockSize) {
        pipeline.activeSampleRate.store(sampleRate, std::memory_order_release);
        pipeline.activeBlockSize.store(blockSize, std::memory_order_release);
        pipeline.fadeStep =
            static_cast<float>(1.0 / (sampleRate * AudioRenderPipeline::kFadeInSeconds));
        pipeline.currentGainLinear = 0.0f;
        pipeline.resetGainOnNextCallback.store(true, std::memory_order_release);
    }

    /// Ceiling the safety processing applies to the final output.
    [[nodiscard]] static constexpr float limiterCeiling() {
        return AudioRenderPipeline::kLimiterCeiling;
    }
};

}  // namespace riffra
