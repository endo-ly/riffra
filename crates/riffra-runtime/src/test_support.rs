use crate::BuiltInInstrumentCatalog;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn prepare_empty_built_in_resource_root(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("manifest.json"),
        br#"{"sourceRelease":"vtest","presets":[]}"#,
    )
    .unwrap();
}

pub(crate) fn prepare_built_in_resource_root(root: &Path) -> PathBuf {
    prepare_empty_built_in_resource_root(root);
    root.to_path_buf()
}

pub(crate) fn empty_built_in_catalog() -> &'static BuiltInInstrumentCatalog {
    static CATALOG: OnceLock<BuiltInInstrumentCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "riffra-runtime-empty-builtins-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        prepare_empty_built_in_resource_root(&root);
        BuiltInInstrumentCatalog::load(root).unwrap()
    })
}

/// Decodes a wire command, as a Host does for every request.
pub(crate) fn command(name: &str, params: serde_json::Value) -> crate::api::ControlCommand {
    crate::api::ControlCommand::decode(name, params).unwrap()
}

/// Returns the wire `type` of a dispatched result.
pub(crate) fn output_type(result: &crate::DispatchResult) -> String {
    serde_json::to_value(&result.output).unwrap()["type"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Returns the wire `value` of a dispatched result.
pub(crate) fn output_value(result: &crate::DispatchResult) -> serde_json::Value {
    serde_json::to_value(&result.output).unwrap()["value"].take()
}

/// Returns the canonical session committed by a mutation result.
pub(crate) fn mutated_session(result: &crate::DispatchResult) -> riffra_core::CreativeSession {
    match &result.output {
        crate::api::ControlOutput::ArrangementMutation(mutation) => {
            mutation.canonical.session.clone()
        }
        output => panic!("expected an arrangement mutation, got {output:?}"),
    }
}
