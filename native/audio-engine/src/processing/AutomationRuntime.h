#pragma once

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <utility>
#include <vector>

namespace riffra {

/// Prepared automation lane evaluated by a forward-only realtime cursor.
class AutomationRuntime final {
public:
    struct Point final {
        std::int64_t sample = 0;
        float value = 0.0f;
    };

    struct Block final {
        float start = 0.0f;
        float increment = 0.0f;
        bool automated = false;
    };

    class Cursor final {
    public:
        Cursor(const AutomationRuntime& lane, const std::int64_t sample) noexcept
            : points(lane.points), index(lane.indexFor(sample)), lastSample(sample - 1) {}

        [[nodiscard]] float valueAt(const std::int64_t sample, const float fallback) noexcept {
            if (sample < lastSample) {
                index = lowerBound(sample);
            }
            while (index < points.size() && points[index].sample <= sample) ++index;
            lastSample = sample;
            if (points.empty()) return fallback;
            if (index == 0) return points.front().value;
            if (index >= points.size()) return points.back().value;
            const auto& left = points[index - 1];
            const auto& right = points[index];
            const auto distance = right.sample - left.sample;
            if (distance <= 0) return right.value;
            const auto amount =
                static_cast<float>(sample - left.sample) / static_cast<float>(distance);
            return left.value + (right.value - left.value) * amount;
        }

    private:
        [[nodiscard]] std::size_t lowerBound(const std::int64_t sample) const noexcept {
            const auto found = std::lower_bound(
                points.begin(), points.end(), sample,
                [](const Point& point, const std::int64_t value) { return point.sample < value; });
            return static_cast<std::size_t>(found - points.begin());
        }

        const std::vector<Point>& points;
        std::size_t index = 0;
        std::int64_t lastSample = -1;
    };

    void setPoints(std::vector<Point> next) noexcept {
        std::sort(next.begin(), next.end(),
                  [](const Point& left, const Point& right) { return left.sample < right.sample; });
        points = std::move(next);
    }

    [[nodiscard]] bool empty() const noexcept { return points.empty(); }

    [[nodiscard]] Cursor cursorAt(const std::int64_t sample) const noexcept {
        return Cursor(*this, sample);
    }

    [[nodiscard]] Block block(const std::int64_t sample, const int sampleCount,
                              const float fallback) const noexcept {
        if (points.empty() || sampleCount <= 0) return {fallback, 0.0f, false};
        auto cursor = cursorAt(sample);
        const auto start = cursor.valueAt(sample, fallback);
        const auto end = cursor.valueAt(sample + sampleCount, fallback);
        return {start, (end - start) / static_cast<float>(sampleCount), true};
    }

private:
    [[nodiscard]] std::size_t indexFor(const std::int64_t sample) const noexcept {
        const auto found = std::upper_bound(
            points.begin(), points.end(), sample,
            [](const std::int64_t value, const Point& point) { return value < point.sample; });
        return static_cast<std::size_t>(found - points.begin());
    }

    std::vector<Point> points;
};

}  // namespace riffra
