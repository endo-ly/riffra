#pragma once

namespace riffra {

[[nodiscard]] bool deviceLossRequiresFault(bool devicePresent,
                                           bool deviceTransitionActive) noexcept;

}  // namespace riffra
