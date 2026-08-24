//! Thin Tauri command boundary for shared Session operations.
//!
//! Each command receives an `AppHandle`, moves synchronous work to the
//! blocking pool, and builds a
//! [`SessionContext`](riffra_runtime::session::context::SessionContext) of concrete
//! dependencies, delegates to the matching Core operation, and returns
//! the resulting DTO. The production workflow (arrangement edit, runtime
//! sync, validate/persist) is hosted by the shared Runtime adapter, which delegates
//! canonical edits to riffra-core; nothing here re-implements it.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::missing::MissingDependency;
use crate::model::{ArrangementMutationResult, RuntimeProjectionStatus, SessionAudioPair};
use crate::storage::SessionStore;
use riffra_core::application::{
    MidiNoteInput, MidiNotePatch, MidiNoteUpdate, SessionSettingsPatch,
};
use riffra_core::{
    AssetId, AudioClipMove, AudioClipPatch, AudioTakeVariant, AutomationParameter, AutomationPoint,
    FrameRange, HistoryState, MidiClipMove, MidiClipPatch, MidiInputRoute, ProjectTimebase,
    TimelineTick, TrackKind,
};
use riffra_runtime::session::{adapter, context::SessionContext};

async fn run_blocking<T, E, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _command_gate = state.host.lock_command_gate()?;
        operation(state.inner()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Session blocking operation failed: {error}"))?
}

async fn run_blocking_without_command_gate<T, E, F>(
    app: AppHandle,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Session blocking operation failed: {error}"))?
}

/// Runtime controls must not queue behind canonical Session persistence or a
/// slow VST/native operation. They only read the current snapshot when needed
/// and never mutate the durable Session.
async fn run_runtime_control<T, E, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Runtime control operation failed: {error}"))?
}

fn app_context(state: &AppState) -> SessionContext<'_> {
    SessionContext {
        core: state.host.core(),
        audio: state.host.core().audio(),
        runtime: state.host.runtime(),
        data_root: state.host.data_root(),
        safe_mode: state.host.core().safe_mode(),
        events: state.host.event_sink(),
    }
}

fn validate_target_instrument_track(state: &AppState, track_id: &str) -> Result<(), String> {
    if state.host.core().safe_mode() {
        return Err("Safe Mode does not allow targeted MIDI input.".into());
    }
    if track_id.trim().is_empty() {
        return Err("A target track is required for targeted MIDI.".into());
    }
    let session = state
        .host
        .core()
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let track = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("The target Track is not registered: {track_id}"))?;
    if track.kind != TrackKind::Instrument {
        return Err("Targeted MIDI input requires an Instrument Track.".into());
    }
    if track.instrument.is_none() {
        return Err("The target Instrument Track has no assigned instrument.".into());
    }
    Ok(())
}

mod arrangement;
mod project;
mod rack;
mod transport;

pub(crate) use arrangement::*;
pub(crate) use project::*;
pub(crate) use rack::*;
pub(crate) use transport::*;
