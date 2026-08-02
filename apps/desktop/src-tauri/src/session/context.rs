use crate::native_audio::AudioSupervisor;
use crate::runtime::RuntimeReconciler;
use crate::runtime::ports::RuntimeDriver;
use crate::session::CreativeSession;
use crate::session::actor::SessionActor;
use std::path::Path;
use std::sync::Mutex;

/// Concrete dependencies shared by Session application operations. Keeping the
/// context separate prevents commit and Transport modules from importing the
/// entire Session application implementation.
pub struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<D>,
    pub session_actor: &'a SessionActor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

pub(crate) fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}
