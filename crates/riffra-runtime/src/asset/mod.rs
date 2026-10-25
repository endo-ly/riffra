pub(crate) mod application;

pub(crate) use riffra_host::discard_output_registration;
pub use riffra_host::{
    ensure_assets_schema, load, register, register_derived, registered_render_asset_count,
    relocate_content_location, remove_render_assets, render_asset_dependents,
    render_asset_location_owners, resolve_content_location, update_metadata,
};
