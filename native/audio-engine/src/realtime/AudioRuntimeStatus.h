#pragma once

namespace riffra {

[[nodiscard]] bool deviceLossRequiresFault(bool devicePresent, bool audioActive) noexcept;

} // namespace riffra
