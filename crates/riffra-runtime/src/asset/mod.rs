pub mod application;

pub use riffra_host::{
    ensure_assets_schema, load, register, register_derived, relocate_content_location,
    resolve_audio_path, resolve_content_location, update_metadata,
};
