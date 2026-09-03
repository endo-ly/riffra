//! Tauri adapters for Host-owned Session operations.

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::model::{
    ArrangementMutationResult, AudioStatus, RuntimeProjectionStatus, SessionAudioPair,
};
use riffra_core::application::{
    MidiNoteInput, MidiNotePatch, MidiNoteUpdate, SessionSettingsPatch,
};
use riffra_core::{
    AssetId, AudioClipMove, AudioClipPatch, AudioTakeVariant, AutomationParameter, AutomationPoint,
    FrameRange, HistoryState, MidiClipMove, MidiClipPatch, MidiInputRoute, ProjectTimebase,
    TimelineTick, TrackKind,
};
use riffra_runtime::missing::MissingDependency;

pub(super) async fn dispatch<T, P>(
    app: AppHandle,
    command: &'static str,
    params: P,
) -> Result<T, String>
where
    T: DeserializeOwned + Send + 'static,
    P: Serialize + Send + 'static,
{
    let params = serde_json::to_value(params).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch(command, params)
    })
    .await
    .map_err(|error| format!("Host operation failed: {error}"))?
}

pub(super) async fn dispatch_json<T: DeserializeOwned + Send + 'static>(
    app: AppHandle,
    command: &'static str,
    params: Value,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch(command, params)
    })
    .await
    .map_err(|error| format!("Host operation failed: {error}"))?
}

mod arrangement;
mod project;
mod rack;
mod transport;

pub(crate) use arrangement::*;
pub(crate) use project::*;
pub(crate) use rack::*;
pub(crate) use transport::*;
