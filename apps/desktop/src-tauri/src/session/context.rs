use crate::native_audio::AudioSupervisor;
use crate::presentation::DesktopViewState;
use crate::runtime::RuntimeReconciler;
use crate::runtime::ports::RuntimeDriver;
use riffra_core::AppCore;
use std::path::Path;
use std::sync::Mutex;

/// Concrete dependencies shared by Session application operations. Keeping the
/// context separate prevents commit and Transport modules from importing the
/// entire Session application implementation.
pub struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub core: &'a AppCore<AudioSupervisor>,
    pub view_state: &'a Mutex<DesktopViewState>,
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<D>,
    pub data_root: &'a Path,
    pub safe_mode: bool,
}

pub(crate) fn current_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::session::CreativeSession, String> {
    context
        .core
        .snapshot()
        .map(|snapshot| snapshot.session)
        .map_err(|error| error.to_string())
}

pub(crate) fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    tracing::error!(%message, "aborting after a poisoned state lock");
    std::process::abort();
}
