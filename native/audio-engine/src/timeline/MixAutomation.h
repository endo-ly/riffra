#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <cmath>
#include <limits>

#include "AutomationRuntime.h"

namespace riffra {

/// Sample-accurate dB and pan ramps with constant work between point boundaries.
class MixAutomationCursor final {
public:
    struct Gains {
        float left;
        float right;
    };

    MixAutomationCursor(const AutomationRuntime& gainLane, const AutomationRuntime& panLane,
                        const std::int64_t start, const int count, const float gainDb,
                        const float pan, const bool unityCenterStereo, const bool active) noexcept
        : gainCursor(gainLane.cursorAt(start)),
          panCursor(panLane.cursorAt(start)),
          sample(start),
          end(start + count),
          staticGain(gainDb),
          staticPan(pan),
          scale(unityCenterStereo ? std::sqrt(2.0f) : 1.0f),
          gainAutomated(active && !gainLane.empty()),
          panAutomated(active && !panLane.empty()) {}

    [[nodiscard]] Gains next() noexcept {
        if (sample >= segmentEnd) prepareSegment();
        const Gains result{gain * std::min(1.0f, cosine * scale),
                           gain * std::min(1.0f, sine * scale)};
        gain *= gainRatio;
        const auto nextCosine = cosine * deltaCosine - sine * deltaSine;
        sine = sine * deltaCosine + cosine * deltaSine;
        cosine = nextCosine;
        ++sample;
        return result;
    }

private:
    void prepareSegment() noexcept {
        auto volume = gainAutomated ? gainCursor.segmentAt(sample, end, staticGain)
                                    : AutomationRuntime::Segment{end, staticGain, staticGain};
        auto pan = panAutomated ? panCursor.segmentAt(sample, end, staticPan)
                                : AutomationRuntime::Segment{end, staticPan, staticPan};
        segmentEnd = std::min(volume.endSample, pan.endSample);
        if (gainAutomated) volume = gainCursor.segmentAt(sample, segmentEnd, staticGain);
        if (panAutomated) pan = panCursor.segmentAt(sample, segmentEnd, staticPan);
        const auto count = static_cast<float>(std::max<std::int64_t>(1, segmentEnd - sample));
        gain = juce::Decibels::decibelsToGain(volume.startValue);
        gainRatio = std::pow(10.0f, (volume.endValue - volume.startValue) / (20.0f * count));
        const auto angle = (pan.startValue + 1.0f) * juce::MathConstants<float>::pi * 0.25f;
        const auto delta =
            (pan.endValue - pan.startValue) * juce::MathConstants<float>::pi * 0.25f / count;
        cosine = std::cos(angle);
        sine = std::sin(angle);
        deltaCosine = std::cos(delta);
        deltaSine = std::sin(delta);
    }

    AutomationRuntime::Cursor gainCursor;
    AutomationRuntime::Cursor panCursor;
    std::int64_t sample;
    std::int64_t end;
    std::int64_t segmentEnd = std::numeric_limits<std::int64_t>::min();
    float staticGain, staticPan, scale;
    bool gainAutomated, panAutomated;
    float gain = 1.0f, gainRatio = 1.0f;
    float cosine = 1.0f, sine = 0.0f, deltaCosine = 1.0f, deltaSine = 0.0f;
};

}  // namespace riffra
