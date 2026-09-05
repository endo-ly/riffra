use crate::BuiltInInstrumentCatalog;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn prepare_empty_built_in_resource_root(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("manifest.json"),
        br#"{"sourceRevision":"test-revision","presets":[]}"#,
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
