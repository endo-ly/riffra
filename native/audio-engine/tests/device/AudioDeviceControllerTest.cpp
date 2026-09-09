#include <gtest/gtest.h>

#include "device/AudioDeviceController.h"

namespace riffra {
namespace {

TEST(AudioDeviceControllerTest, DeviceLossRequiresFaultOnlyOutsideTransition) {
    EXPECT_TRUE(AudioDeviceController::requiresFaultForState(false, false));
    EXPECT_FALSE(AudioDeviceController::requiresFaultForState(false, true));
    EXPECT_FALSE(AudioDeviceController::requiresFaultForState(true, false));
}

}  // namespace
}  // namespace riffra
