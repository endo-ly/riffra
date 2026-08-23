//! Shared local control contract used by the Desktop server and CLI client.
//!
//! This crate owns only protocol, endpoint, framing, and local transport
//! concerns. It deliberately has no dependency on the production Core or on
//! any Desktop runtime implementation.

mod command;
mod endpoint;
mod protocol;
pub mod transport;

pub use command::ControlCommand;
pub use endpoint::{
    EndpointDescriptor, endpoint_path, new_instance_id, pipe_name, publish_endpoint, read_endpoint,
    remove_endpoint_if_matches,
};
pub use protocol::{
    CommandResult, ControlRequest, ControlResponse, ErrorCode, HelloRequest, HelloResponse,
    PROTOCOL_VERSION, ProtocolError,
};
