//! Shared operating-system host infrastructure for Riffra.
//!
//! This crate owns durable production data and file-format boundaries used by
//! both the Desktop and standalone CLI hosts. It deliberately contains no
//! Tauri, WebView, audio-runtime, or plugin-lifecycle code.

mod asset;
mod audio_file;
mod data_root;
mod midi_file;
mod project;
mod project_store;
mod storage;

pub use asset::{
    ensure_assets_schema, import_midi_asset, import_midi_bytes, load, register, register_derived,
    relocate_content_location, resolve_audio_path, resolve_content_location, update_metadata,
};
pub use audio_file::{WavMetadata, parse_wav};
pub use data_root::DataRootLease;
pub use midi_file::parse_smf;
pub use project::{ProjectExport, export as export_project, import as import_project};
pub use project_store::{
    ProjectInitialization, ProjectStore, ProjectSummary, WorkspaceState, validate_project_id,
};
pub use storage::{
    LoadedSession, RecoveryCandidate, SessionLoadError, SessionStore, now_ms, replace_file,
};
