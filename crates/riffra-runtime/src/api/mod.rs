//! The Control Command contract: what a Host can be asked to do.
//!
//! [`table`] names every command once, with its params, result, and policy.
//! This module defines requests and results only; executing them is the job
//! of the Standalone dispatcher and the live Host.

mod decode;
pub mod output;
pub mod params;
mod policy;
mod table;

pub use decode::CommandDecodeError;
pub(crate) use decode::{attach_input_value, json_pointer_segment};
pub use output::ControlOutput;
pub use policy::{CanonicalAccess, CommandExecutor, CommandPolicy, CommandScope};
pub use table::{
    CanonicalCommand, ControlCommand, ProjectCommand, RuntimeCommand, typescript_result_map,
};
