//! project command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(command, "project.export" | "project.import")
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "project.export" => dispatcher.value(
            "projectExport",
            riffra_host::export_project(&dispatcher.data_root, &canonical.session, now_ms())?,
        ),
        "project.import" => {
            let params: ProjectImportParams = decode(request.params)?;
            let session = riffra_host::import_project(&dispatcher.data_root, &params.path)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .import_project(session)?,
            )
        }
        _ => unreachable!("unsupported project command family"),
    })
}

#[derive(Debug, Deserialize)]
struct ProjectImportParams {
    path: PathBuf,
}
