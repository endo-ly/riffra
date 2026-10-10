//! Parameters of Control Commands.
//!
//! Every params struct rejects unknown keys so that a misspelled field fails
//! with its JSON path instead of being ignored.

mod clips;
mod device;
mod instrument;
mod instrument_event;
mod library;
mod music;
mod project;
mod record;
mod runtime;
mod session;
mod track;

pub use clips::*;
pub use device::*;
pub use instrument::*;
pub use instrument_event::*;
pub use library::*;
pub use music::*;
pub use project::*;
pub use record::*;
pub use runtime::*;
pub use session::*;
pub use track::*;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Params of a command that takes no parameters.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

/// Identifies one Track.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackIdParams {
    pub track_id: String,
}

/// Identifies one timeline Clip.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClipIdParams {
    pub clip_id: String,
}

/// Identifies one Track Device anywhere in the arrangement.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceIdParams {
    pub device_id: String,
}

/// Identifies one Device on one Track.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackDeviceParams {
    pub track_id: String,
    pub device_id: String,
}

/// Identifies one recording take, background job, or Library asset.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdParams {
    pub id: String,
}
