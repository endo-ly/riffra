#pragma once

#include <string_view>

namespace riffra {

enum class CommandFamily {
    shutdown,
    safety,
    timeline,
    trackDevice,
    transport,
    midi,
    preview,
    device,
    recording,
    status,
    unsupported,
};

[[nodiscard]] CommandFamily commandFamilyFor(std::string_view type) noexcept;

}  // namespace riffra
