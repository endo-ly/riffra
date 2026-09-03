//! Shared adapters for Core Session operations and Host runtime services.
//!
//! The operations use two consistency policies:
//!
//! - Arrangement operations that change plugin topology prepare the proposed
//!   runtime graph before persisting the canonical Session. A failed candidate
//!   is rejected, and a persistence failure restores the previous graph. Other
//!   Arrangement operations commit first and submit a nonblocking projection.
//!
//! - Pure-session operations ([`restore_generation`])
//!   mutate the session and persist it without waiting for VST lifecycle work.
//!
//! Core owns editing rules, canonical state, history, and conflict decisions.
//! This layer resolves files and plugins, invokes native services, delegates
//! production changes to Core, and compensates host resources when an external
//! operation fails.

mod rack;
mod recording;
mod runtime;

pub use rack::*;
pub use recording::*;
pub use runtime::*;

use std::path::Path;

use crate::RuntimeDriver;
use crate::asset;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::plugins;
use riffra_core::{AssetId, AssetKind, AudioTakeVariant, MidiInputRoute};

pub use crate::session::commit::{
    arrangement_mutation_result, arrangement_mutation_without_projection, commit_core_application,
    publish_canonical_state, restore_generation,
};
pub use crate::session::context::{SessionContext, current_session};
pub use crate::session::error::AdapterError;
pub use crate::session::transport::{
    go_to_start_timeline, play_timeline, prepare_arrangement_candidate, seek_timeline,
    stop_timeline, sync_arrangement_runtime,
};
use riffra_core::application::SessionSettingsPatch;

pub fn undo(
    context: &SessionContext<'_>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let session = context
        .core
        .application(&context.storage)
        .undo()
        .map_err(AdapterError::from)?;
    crate::library::index::refresh(context.data_root, &context.storage, &session);
    publish_canonical_state(context)?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn redo(
    context: &SessionContext<'_>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let session = context
        .core
        .application(&context.storage)
        .redo()
        .map_err(AdapterError::from)?;
    crate::library::index::refresh(context.data_root, &context.storage, &session);
    publish_canonical_state(context)?;
    crate::session::adapter::arrangement_mutation_result(context)
}
