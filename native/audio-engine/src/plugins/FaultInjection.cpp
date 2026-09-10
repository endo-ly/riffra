#include "FaultInjection.h"

#include <atomic>
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>

#if defined(_WIN32)
#include <windows.h>
#endif

namespace riffra {

namespace {

std::string environmentValue(const char* name) {
#if defined(_WIN32)
    char* value = nullptr;
    std::size_t length = 0;
    if (_dupenv_s(&value, &length, name) != 0 || value == nullptr) return {};
    const std::string result(value, length > 0 ? length - 1 : 0);
    std::free(value);
    return result;
#else
    const auto* value = std::getenv(name);
    return value != nullptr ? value : "";
#endif
}

const char* stageName(const FaultStage stage) noexcept {
    switch (stage) {
        case FaultStage::discovery:
            return "discovery";
        case FaultStage::create:
            return "create";
        case FaultStage::prepare:
            return "prepare";
        case FaultStage::stateApply:
            return "stateApply";
        case FaultStage::editorOpen:
            return "editorOpen";
        case FaultStage::destroy:
            return "destroy";
    }
    return "";
}

bool stageSelected(const FaultStage stage) noexcept {
    const auto configured = environmentValue("RIFFRA_FAULT_STAGE");
    return configured.empty() || configured == stageName(stage);
}

int delayMilliseconds() noexcept {
    const auto configured = environmentValue("RIFFRA_FAULT_DELAY_MS");
    const auto parsed = std::strtol(configured.c_str(), nullptr, 10);
    return parsed > 0 && parsed <= 300'000 ? static_cast<int>(parsed) : 5'000;
}

bool modeMatches(const char* mode, const FaultStage stage) noexcept {
    const auto configured = environmentValue("RIFFRA_FAULT_MODE");
    if (configured == mode) return true;
    if (configured == std::string(stageName(stage)) + "Delay") return std::string(mode) == "delay";
    return false;
}

void abortProcess() noexcept {
#if defined(_WIN32)
    ::TerminateProcess(::GetCurrentProcess(), 97);
#else
    std::_Exit(97);
#endif
}

}  // namespace

void FaultInjection::before(const FaultStage stage) {
    if (!stageSelected(stage)) return;
    const auto mode = environmentValue("RIFFRA_FAULT_MODE");
    if (mode.empty()) return;
    if (modeMatches("throwException", stage))
        throw std::runtime_error("Fault injection requested at the " +
                                 std::string(stageName(stage)) + " boundary.");
    if (modeMatches("processAbort", stage)) abortProcess();
    if (modeMatches("neverReturn", stage)) {
        for (;;) std::this_thread::sleep_for(std::chrono::seconds(1));
    }
    if (modeMatches("delay", stage))
        std::this_thread::sleep_for(std::chrono::milliseconds(delayMilliseconds()));
}

void FaultInjection::stdoutFlood() {
    if (environmentValue("RIFFRA_FAULT_MODE") != "stdoutFlood") return;
    static std::atomic<bool> emitted{false};
    if (emitted.exchange(true, std::memory_order_acq_rel)) return;
    std::string flood(256 * 1024, 'x');
    flood.push_back('\n');
    std::cout << flood << std::flush;
}

}  // namespace riffra
