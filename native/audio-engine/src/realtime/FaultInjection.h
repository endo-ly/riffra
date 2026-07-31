#pragma once

namespace riffra {

enum class FaultStage {
    discovery,
    create,
    prepare,
    stateApply,
    editorOpen,
    destroy,
};

/// Test-only lifecycle faults are enabled through environment variables so a
/// real third-party VST is not required to reproduce a stalled boundary.
///
/// RIFFRA_FAULT_STAGE selects a lifecycle boundary (discovery, create,
/// prepare, stateApply, editorOpen, or destroy). RIFFRA_FAULT_MODE accepts a
/// stage delay name such as prepareDelay, or a global action: delay,
/// neverReturn, throwException, processAbort, or stdoutFlood. The delay is
/// controlled by RIFFRA_FAULT_DELAY_MS and defaults to 5000 milliseconds.
class FaultInjection final {
public:
    static void before(FaultStage stage);
    static void stdoutFlood();
};

}  // namespace riffra
