//! Typed execution projections shared by live playback and offline rendering.

mod contract;
mod project;
mod resolve;

pub(crate) use contract::*;
pub(crate) use project::{project, project_graph};
pub(crate) use resolve::{ResolvedResources, resolve};

use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::CreativeSession;
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// Resources missing from an executable projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDiagnostics {
    pub unavailable_clip_ids: Vec<String>,
    pub missing_device_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedTimeline {
    pub(crate) snapshot: TimelineSnapshot,
    pub(crate) diagnostics: ProjectionDiagnostics,
}

/// Resolves external resources and projects one canonical session for live playback.
pub(crate) fn project_session(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    project_id: &str,
    session: &CreativeSession,
) -> ProjectedTimeline {
    let resources = resolve(data_root, catalog, session);
    project(project_id, session, &resources)
}
