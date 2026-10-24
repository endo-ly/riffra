#include "MetronomeClicks.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <numbers>
#include <span>

namespace riffra {

namespace {

/// One exponentially decaying sine above the click's fundamental.
struct Partial final {
    double ratio = 1.0;
    double amplitude = 1.0;
    double decaySeconds = 0.0;
};

/// A struck click: damped partials plus an optional lowpassed noise burst for the strike.
struct ClickRecipe final {
    std::span<const Partial> partials;
    double accentHz = 0.0;
    double beatHz = 0.0;
    double attackSeconds = 0.0;
    double noiseAmplitude = 0.0;
    double noiseDecaySeconds = 0.0;
    /// Calibrated so every sound's accent has the same A-weighted loudness.
    float gain = 1.0f;
};

/// Decay time constants rendered before a click ends, leaving its slowest partial near -70 dB.
constexpr double kTimeConstantsPerClick = 8.0;
constexpr double kNoiseCutoffHz = 9'000.0;
/// Beat clicks sit 2.5 dB below the accent.
constexpr float kBeatGain = 0.75f;

constexpr std::array kBeepPartials{Partial{1.0, 1.0, 0.015}};
constexpr std::array kWoodPartials{Partial{1.0, 1.0, 0.025}, Partial{2.71, 0.45, 0.010},
                                   Partial{4.6, 0.25, 0.005}};

constexpr ClickRecipe kBeep{kBeepPartials, 1'600.0, 1'000.0, 0.001, 0.0, 0.0, 0.48f};
constexpr ClickRecipe kWood{kWoodPartials, 1'300.0, 900.0, 0.0003, 1.2, 0.0015, 0.36f};

const ClickRecipe& recipeFor(const MetronomeSoundSpec sound) noexcept {
    switch (sound) {
        case MetronomeSoundSpec::beep:
            return kBeep;
        case MetronomeSoundSpec::wood:
            return kWood;
    }
    return kBeep;
}

std::vector<float> renderClick(const ClickRecipe& recipe, const double fundamentalHz,
                               const float gain, const double sampleRate) {
    const auto slowestDecay = std::max_element(recipe.partials.begin(), recipe.partials.end(),
                                               [](const Partial& left, const Partial& right) {
                                                   return left.decaySeconds < right.decaySeconds;
                                               })
                                  ->decaySeconds;
    std::vector<float> click(
        static_cast<std::size_t>(std::lround(slowestDecay * kTimeConstantsPerClick * sampleRate)));
    const auto noiseFeedback = std::exp(-2.0 * std::numbers::pi * kNoiseCutoffHz / sampleRate);
    std::uint32_t noiseState = 0x9E37'79B9u;
    double noise = 0.0;
    for (std::size_t index = 0; index < click.size(); ++index) {
        const auto time = static_cast<double>(index) / sampleRate;
        double value = 0.0;
        for (const auto& partial : recipe.partials)
            value += partial.amplitude *
                     std::sin(2.0 * std::numbers::pi * fundamentalHz * partial.ratio * time) *
                     std::exp(-time / partial.decaySeconds);
        if (recipe.noiseAmplitude > 0.0) {
            noiseState ^= noiseState << 13;
            noiseState ^= noiseState >> 17;
            noiseState ^= noiseState << 5;
            const auto white = static_cast<double>(noiseState) / 2'147'483'648.0 - 1.0;
            noise = (1.0 - noiseFeedback) * white + noiseFeedback * noise;
            value += recipe.noiseAmplitude * noise * std::exp(-time / recipe.noiseDecaySeconds);
        }
        click[index] =
            static_cast<float>(value * std::min(time / recipe.attackSeconds, 1.0)) * gain;
    }
    return click;
}

}  // namespace

MetronomeClicks renderMetronomeClicks(const MetronomeSoundSpec sound, const double sampleRate) {
    const auto& recipe = recipeFor(sound);
    return {renderClick(recipe, recipe.accentHz, recipe.gain, sampleRate),
            renderClick(recipe, recipe.beatHz, recipe.gain * kBeatGain, sampleRate)};
}

}  // namespace riffra
