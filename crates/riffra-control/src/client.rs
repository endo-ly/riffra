use crate::registry::LocalHostRegistration;
use crate::transport::{self, ReadWrite, TransportError};
use crate::{
    ControlRequest, ControlResponse, EndpointDescriptor, HelloRequest, HelloResponse,
    HostEventFrame,
};
use std::path::Path;
use std::time::Duration;

/// Upper bound for one command round trip against an attached Host. Heavy
/// operations such as plugin scans finish well inside this bound, while a
/// silent Host must not block Desktop switching forever.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// Upper bound for the initial hello exchange of one connection.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Client for one verified local Host that opens a fresh command connection
/// per request so long operations cannot block unrelated commands.
#[derive(Clone, Debug)]
pub struct LocalHostClient {
    descriptor: EndpointDescriptor,
}

impl LocalHostClient {
    /// Connects to the Host descriptor published below a Data Root.
    pub fn connect_data_root(data_root: &Path) -> Result<Self, LocalHostClientError> {
        let descriptor =
            crate::read_endpoint(data_root).map_err(LocalHostClientError::Discovery)?;
        Ok(Self { descriptor })
    }

    /// Targets a registry entry and verifies its instance identity per request.
    pub fn connect_registration(registration: &LocalHostRegistration) -> Self {
        Self {
            descriptor: registration.descriptor(),
        }
    }

    /// Returns the descriptor verified during every command handshake.
    pub fn descriptor(&self) -> &EndpointDescriptor {
        &self.descriptor
    }

    /// Sends one command over its own connection and waits for its response.
    ///
    /// # Errors
    /// Fails when the endpoint is unreachable, the handshake identity does not
    /// match, or no response arrives within [`COMMAND_TIMEOUT`].
    pub fn request(
        &self,
        request: &ControlRequest,
    ) -> Result<ControlResponse, LocalHostClientError> {
        request
            .validate()
            .map_err(|error| LocalHostClientError::InvalidRequest(error.message))?;
        let mut stream = self.open_command_stream()?;
        transport::write_frame(&mut *stream, request)?;
        let response: ControlResponse = transport::read_frame_within(&*stream, COMMAND_TIMEOUT)?;
        if response.request_id != request.request_id {
            return Err(LocalHostClientError::ResponseMismatch {
                expected: request.request_id.clone(),
                actual: response.request_id,
            });
        }
        Ok(response)
    }

    /// Opens a separate long-lived event connection so commands and events
    /// cannot race on one request/response stream.
    pub fn open_event_stream(&self) -> Result<LocalHostEventStream, LocalHostClientError> {
        LocalHostEventStream::connect(self.descriptor.clone())
    }

    fn open_command_stream(&self) -> Result<Box<dyn ReadWrite>, LocalHostClientError> {
        let mut stream = transport::connect(self.descriptor.endpoint())?;
        transport::write_frame(&mut *stream, &HelloRequest::command())?;
        let hello: HelloResponse = transport::read_frame_within(&*stream, HANDSHAKE_TIMEOUT)?;
        verify_hello(&self.descriptor, hello)?;
        Ok(stream)
    }
}

/// Server-to-client event stream for one Host connection.
pub struct LocalHostEventStream {
    descriptor: EndpointDescriptor,
    stream: Box<dyn ReadWrite>,
}

/// A handle that can interrupt a blocked local event-stream reader.
///
/// Closing alone may leave a synchronous Windows pipe read blocked; pair it
/// with [`transport::cancel_synchronous_io`] on the reader thread.
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
        let hello: HelloResponse = transport::read_frame_within(&*stream, HANDSHAKE_TIMEOUT)?;
        verify_hello(&descriptor, hello)?;
        Ok(Self { descriptor, stream })
    }

    /// Blocks until the next Host event frame or the event connection closes.
    pub fn recv(&mut self) -> Result<HostEventFrame, LocalHostClientError> {
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
    hello: HelloResponse,
) -> Result<(), LocalHostClientError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnectionRole, new_instance_id, transport::LocalControlListener};
    use std::thread;
    use std::time::Instant;

    #[test]
    fn event_reader_stops_in_finite_time_when_the_connection_closes() {
        let descriptor = EndpointDescriptor::new(new_instance_id(), std::process::id());
        let mut listener = LocalControlListener::bind(descriptor.endpoint()).unwrap();
        let server_descriptor = descriptor.clone();
        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let hello: HelloRequest = transport::read_frame(&mut stream).unwrap();
            assert_eq!(hello.role, ConnectionRole::Events);
            transport::write_frame(
                &mut stream,
                &HelloResponse::new(server_descriptor.instance_id, server_descriptor.pid),
            )
            .unwrap();
            // Hold the connection open without ever sending an event.
            thread::sleep(Duration::from_secs(10));
        });

        let mut events = LocalHostEventStream::connect(descriptor).unwrap();
        let closer = events.close_handle().unwrap();
        let reader = thread::Builder::new()
            .name("riffra-test-event-reader".into())
            .spawn(move || {
                loop {
                    if events.recv().is_err() {
                        break;
                    }
                }
            })
            .unwrap();
        // Give the reader time to enter its blocking read before closing.
        thread::sleep(Duration::from_millis(200));

        closer.close();
        #[cfg(windows)]
        transport::cancel_synchronous_io(&reader);

        let deadline = Instant::now() + Duration::from_secs(10);
        while !reader.is_finished() {
            assert!(
                Instant::now() < deadline,
                "the event reader did not stop within its deadline"
            );
            thread::sleep(Duration::from_millis(20));
        }
        reader.join().unwrap();
        server.join().unwrap();
    }
}
