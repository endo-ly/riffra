use crate::native_audio::AudioSupervisor;
use crate::runtime::RuntimeReconciler;
use crate::runtime::ports::RuntimeDriver;
use riffra_core::{AppCore, CreativeSession};
use std::path::Path;
use tauri::AppHandle;

/// Concrete dependencies shared by Session application operations. Keeping the
/// context separate prevents commit and Transport modules from importing the
/// entire Session application implementation.
pub struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub core: &'a AppCore<AudioSupervisor>,
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<D>,
    pub data_root: &'a Path,
    pub safe_mode: bool,
    pub app_handle: Option<&'a AppHandle>,
}

pub(crate) fn current_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<CreativeSession, String> {
    context
        .core
        .snapshot()
        .map(|snapshot| snapshot.session)
        .map_err(|error| error.to_string())
}
