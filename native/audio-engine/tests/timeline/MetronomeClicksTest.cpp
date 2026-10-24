#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <vector>

#include "timeline/MetronomeClicks.h"

namespace riffra {
namespace {

float peak(const std::vector<float>& click) {
    float result = 0.0f;
    for (const auto value : click) result = std::max(result, std::abs(value));
    return result;
}

TEST(MetronomeClicksTest, KeepsEachSoundIndependentOfTheSampleRateAndFreeOfEdgeClicks) {
    for (const auto sound : {MetronomeSoundSpec::beep, MetronomeSoundSpec::wood}) {
        // Arrange / Act
        const auto low = renderMetronomeClicks(sound, 44'100.0);
        const auto high = renderMetronomeClicks(sound, 96'000.0);

        // Assert: same duration and level at both rates, silent edges, and a quieter beat.
        for (const auto* clicks : {&low, &high})
            for (const auto* click : {&clicks->accent, &clicks->beat}) {
                ASSERT_FALSE(click->empty());
                EXPECT_EQ(click->front(), 0.0f);
                EXPECT_LT(std::abs(click->back()), 0.001f);
            }
        EXPECT_NEAR(static_cast<double>(low.accent.size()) / 44'100.0,
                    static_cast<double>(high.accent.size()) / 96'000.0, 0.001);
        EXPECT_NEAR(peak(low.accent), peak(high.accent), peak(high.accent) * 0.1f);
        EXPECT_LT(peak(low.beat), peak(low.accent));
    }
}

}  // namespace
}  // namespace riffra
