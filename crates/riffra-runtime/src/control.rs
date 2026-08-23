use crate::host::HostState;
use riffra_control::{
    ControlRequest, EndpointDescriptor, ErrorCode, HelloRequest, HelloResponse,
    LocalControlEndpoint, ProtocolError, new_instance_id, publish_endpoint,
    remove_endpoint_if_matches, transport,
};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

/// Shared local Host control server.
pub(crate) struct ControlServer {
    stop: Arc<AtomicBool>,
    endpoint: LocalControlEndpoint,
    data_root: std::path::PathBuf,
    instance_id: String,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    pub(crate) fn start(state: Arc<HostState>) -> Result<Self, String> {
        let instance_id = new_instance_id();
        let descriptor =
            EndpointDescriptor::for_data_root(&state.data_root, &instance_id, std::process::id());
        let mut listener = transport::LocalControlListener::bind(descriptor.endpoint())
            .map_err(|error| format!("local control endpoint could not bind: {error}"))?;
        publish_endpoint(&state.data_root, &descriptor)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_instance = instance_id.clone();
        let control_data_root = state.data_root.clone();
        let thread = thread::Builder::new()
            .name("riffra-host-control".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok(stream) => {
                            if thread_stop.load(Ordering::Acquire) {
                                break;
                            }
                            let client_state = Arc::clone(&state);
                            let client_instance = thread_instance.clone();
                            let _ = thread::Builder::new()
                                .name("riffra-host-control-client".into())
                                .spawn(move || handle_client(client_state, client_instance, stream));
                        }
                        Err(error) => {
                            if !thread_stop.load(Ordering::Acquire) {
                                tracing::warn!(error = %error, "local Host control listener stopped");
                            }
                            break;
                        }
                    }
                }
                let _ = remove_endpoint_if_matches(&state.data_root, &thread_instance);
            })
            .map_err(|error| {
                let _ = remove_endpoint_if_matches(&control_data_root, &instance_id);
                format!("local control server could not start: {error}")
            })?;
        tracing::info!(path = %riffra_control::endpoint_path(&control_data_root).display(), "Riffra Host control server is ready");
        Ok(Self {
            stop,
            endpoint: descriptor.endpoint,
            data_root: control_data_root,
            instance_id,
            thread: Some(thread),
        })
    }

    pub(crate) fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = transport::connect(&self.endpoint);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = remove_endpoint_if_matches(&self.data_root, &self.instance_id);
    }
}

fn handle_client(
    state: Arc<HostState>,
    instance_id: String,
    mut stream: Box<dyn transport::ReadWrite>,
) {
    let hello: HelloRequest = match transport::read_frame(&mut stream) {
        Ok(hello) => hello,
        Err(error) => {
            tracing::debug!(error = %error, "Host control client closed during handshake");
            return;
        }
    };
    if hello.message_type != "hello" {
        let _ = transport::write_frame(
            &mut stream,
            &riffra_control::ControlResponse::failure(
                String::new(),
                None,
                ProtocolError::new(
                    ErrorCode::InvalidRequest,
                    "Host control handshake was rejected",
                ),
            ),
        );
        return;
    }
    if transport::write_frame(
        &mut stream,
        &HelloResponse::new(instance_id, std::process::id()),
    )
    .is_err()
    {
        return;
    }
    loop {
        let frame: Value = match transport::read_frame(&mut stream) {
            Ok(frame) => frame,
            Err(error) => {
                if matches!(
                    error,
                    transport::TransportError::InvalidUtf8
                        | transport::TransportError::InvalidJson(_)
                ) {
                    let response = riffra_control::ControlResponse::failure(
                        String::new(),
                        None,
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                    );
                    if transport::write_frame(&mut stream, &response).is_ok() {
                        continue;
                    }
                }
                return;
            }
        };
        let request_id = frame
            .get("requestId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let response = match serde_json::from_value::<ControlRequest>(frame) {
            Ok(request) => state.dispatch_request(request),
            Err(error) => riffra_control::ControlResponse::failure(
                request_id,
                None,
                ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
            ),
        };
        if transport::write_frame(&mut stream, &response).is_err() {
            return;
        }
    }
}
