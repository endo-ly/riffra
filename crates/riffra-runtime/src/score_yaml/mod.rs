//! Score YAML interchange.
//!
//! [`import`] reads a Score document into canonical production state;
//! [`export`] writes canonical production state as a Score song folder.

pub(crate) mod export;
pub(crate) mod import;

mod parameters;
mod plugin_source;
