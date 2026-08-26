use crate::registry::LocalHostRegistration;
use crate::transport::{self, ReadWrite, TransportError};
use crate::{
    ControlRequest, ControlResponse, EndpointDescriptor, HelloRequest, HelloResponse,
    HostEventFrame,
};
use std::path::Path;
use std::sync::Mutex;

/// Errors raised while connecting to or using one local Host.
#[derive(Debug, thiserror::Error)]
pub enum LocalHostClientError {
    #[error("local Host endpoint could not be discovered: {0}")]
    Discovery(String),
    #[error("local Host request was invalid: {0}")]
    InvalidRequest(String),
    #[error("local Host transport failed: {0}")]
    Transport(#[from] TransportError),
    #[error("local Host handshake was rejected: {0}")]
    Handshake(String),
    #[error("local Host returned a response for request {actual}, expected {expected}")]
    ResponseMismatch { expected: String, actual: String },
}

/// Shared command connection to one verified local Host.
pub struct LocalHostClient {
    descriptor: EndpointDescriptor,
    command_stream: Mutex<Box<dyn ReadWrite>>,
}

impl std::fmt::Debug for LocalHostClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalHostClient")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl LocalHostClient {
    /// Connects to the Host descriptor published below a Data Root.
    pub fn connect_data_root(data_root: &Path) -> Result<Self, LocalHostClientError> {
        let descriptor =
            crate::read_endpoint(data_root).map_err(LocalHostClientError::Discovery)?;
        Self::connect_descriptor(descriptor)
    }

    /// Connects to a registry entry and verifies its instance identity.
    pub fn connect_registration(
        registration: &LocalHostRegistration,
    ) -> Result<Self, LocalHostClientError> {
        Self::connect_descriptor(registration.descriptor())
    }

    /// Returns the descriptor verified during the command handshake.
    pub fn descriptor(&self) -> &EndpointDescriptor {
        &self.descriptor
    }

    /// Sends one command request and waits for its ordered response.
    pub fn request(
        &self,
        request: &ControlRequest,
    ) -> Result<ControlResponse, LocalHostClientError> {
        request
            .validate()
            .map_err(|error| LocalHostClientError::InvalidRequest(error.message))?;
        let mut stream = self.command_stream.lock().map_err(|_| {
            LocalHostClientError::Handshake("command stream lock was poisoned".into())
        })?;
        transport::write_frame(&mut **stream, request)?;
        let response: ControlResponse = transport::read_frame(&mut **stream)?;
        if response.request_id != request.request_id {
            return Err(LocalHostClientError::ResponseMismatch {
                expected: request.request_id.clone(),
                actual: response.request_id,
            });
        }
        Ok(response)
    }

    /// Opens a separate event connection so commands and events cannot race
    /// on one request/response stream.
    pub fn open_event_stream(&self) -> Result<LocalHostEventStream, LocalHostClientError> {
        LocalHostEventStream::connect(self.descriptor.clone())
    }

    fn connect_descriptor(descriptor: EndpointDescriptor) -> Result<Self, LocalHostClientError> {
        let mut stream = transport::connect(descriptor.endpoint())?;
        transport::write_frame(&mut *stream, &HelloRequest::command())?;
        verify_hello(&descriptor, &mut *stream)?;
        Ok(Self {
            descriptor,
            command_stream: Mutex::new(stream),
        })
    }
}

/// Server-to-client event stream for one Host connection.
pub struct LocalHostEventStream {
    descriptor: EndpointDescriptor,
    stream: Box<dyn ReadWrite>,
}

/// A handle that can interrupt a blocked local event-stream reader.
pub struct LocalHostEventStreamHandle {
    stream: Box<dyn ReadWrite>,
}

impl LocalHostEventStreamHandle {
    /// Closes the associated event connection.
    pub fn close(&self) {
        self.stream.close_stream();
    }
}

impl std::fmt::Debug for LocalHostEventStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalHostEventStream")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl LocalHostEventStream {
    fn connect(descriptor: EndpointDescriptor) -> Result<Self, LocalHostClientError> {
        let mut stream = transport::connect(descriptor.endpoint())?;
        transport::write_frame(&mut *stream, &HelloRequest::events())?;
        verify_hello(&descriptor, &mut *stream)?;
        Ok(Self { descriptor, stream })
    }

    /// Blocks until the next Host event frame or the event connection closes.
    pub fn next(&mut self) -> Result<HostEventFrame, LocalHostClientError> {
        Ok(transport::read_frame(&mut *self.stream)?)
    }

    /// Creates a separate close handle for the event connection.
    pub fn close_handle(&self) -> Result<LocalHostEventStreamHandle, LocalHostClientError> {
        self.stream
            .try_clone_stream()
            .map(|stream| LocalHostEventStreamHandle { stream })
            .map_err(|error| LocalHostClientError::Transport(TransportError::Io(error)))
    }

    /// Returns the Host descriptor verified during the event handshake.
    pub fn descriptor(&self) -> &EndpointDescriptor {
        &self.descriptor
    }
}

fn verify_hello(
    descriptor: &EndpointDescriptor,
    stream: &mut dyn ReadWrite,
) -> Result<(), LocalHostClientError> {
    let hello: HelloResponse = transport::read_frame(stream)?;
    if hello.message_type != "hello" {
        return Err(LocalHostClientError::Handshake(
            "Host control handshake returned an invalid message type".into(),
        ));
    }
    if hello.instance_id != descriptor.instance_id {
        return Err(LocalHostClientError::Handshake(
            "Host control handshake did not match the endpoint instance".into(),
        ));
    }
    if hello.pid != descriptor.pid {
        return Err(LocalHostClientError::Handshake(
            "Host control handshake did not match the endpoint process".into(),
        ));
    }
    Ok(())
}
