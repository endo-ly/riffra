#include "AudioRuntimeStatus.h"

namespace riffra {

bool deviceLossRequiresFault(const bool devicePresent, const bool deviceTransitionActive) noexcept {
    return !devicePresent && !deviceTransitionActive;
}

}  // namespace riffra
