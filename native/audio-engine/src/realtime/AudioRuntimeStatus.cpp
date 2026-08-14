#include "AudioRuntimeStatus.h"

namespace riffra {

bool deviceLossRequiresFault(const bool devicePresent, const bool audioActive) noexcept {
    return !devicePresent && audioActive;
}

}  // namespace riffra
