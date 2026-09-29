//! Typed execution projections shared by live playback and offline rendering.

mod contract;
mod project;
mod resolve;

pub(crate) use contract::*;
pub(crate) use project::{project, project_graph};
pub(crate) use resolve::{ResolvedResources, resolve};

use crate::api::output::ProjectionDiagnostics;
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::CreativeSession;
use std::path::Path;

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
