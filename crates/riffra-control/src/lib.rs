//! Shared local control contract used by Riffra Hosts and CLI clients.
//!
//! This crate owns only protocol, endpoint, framing, and local transport
//! concerns. It deliberately has no dependency on the production Core or on
//! any Host runtime implementation.

mod client;
mod command;
mod endpoint;
mod protocol;
mod registry;
pub mod transport;

pub use client::{
    LocalHostClient, LocalHostClientError, LocalHostEventStream, LocalHostEventStreamHandle,
};
pub use command::ControlCommand;
pub use endpoint::{
    EndpointDescriptor, HostIdentity, LocalControlEndpoint, endpoint_path, new_instance_id,
    pipe_name, publish_endpoint, read_endpoint, remove_endpoint_if_matches,
};
pub use protocol::{
    CommandResult, ConnectionRole, ControlRequest, ControlResponse, ErrorCode, HelloRequest,
    HelloResponse, HostEventFrame, ProtocolError,
};
pub use registry::{LocalHostDiscovery, LocalHostRegistration, LocalHostRegistry, now_ms};
