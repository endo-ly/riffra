use riffra_core::CreativeSession;
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub path: String,
    pub session_id: String,
    pub exported_at_ms: u64,
    pub asset_count: usize,
}

pub fn export(
    data_root: &Path,
    session: &CreativeSession,
    exported_at_ms: u64,
) -> Result<ProjectExport, String> {
    riffra_host::export_project(data_root, session, exported_at_ms).map(|result| ProjectExport {
        path: result.path,
        session_id: result.session_id,
        exported_at_ms: result.exported_at_ms,
        asset_count: result.asset_count,
    })
}

pub fn import(data_root: &Path, path: &Path) -> Result<CreativeSession, String> {
    riffra_host::import_project(data_root, path)
}
