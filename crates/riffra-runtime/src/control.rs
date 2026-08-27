use crate::host::HostState;
use riffra_control::{
    ConnectionRole, ControlRequest, EndpointDescriptor, ErrorCode, HelloRequest, HelloResponse,
    HostIdentity, LocalControlEndpoint, LocalHostRegistration, LocalHostRegistry, ProtocolError,
    publish_endpoint, remove_endpoint_if_matches, transport,
};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};

/// Shared local Host control server.
pub(crate) struct ControlServer {
    stop: Arc<AtomicBool>,
    endpoint: LocalControlEndpoint,
    data_root: std::path::PathBuf,
    instance_id: String,
    registry: LocalHostRegistry,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    pub(crate) fn start(state: Arc<HostState>, identity: HostIdentity) -> Result<Self, String> {
        let instance_id = identity.instance_id.clone();
        let descriptor =
            EndpointDescriptor::for_data_root(&state.data_root, &instance_id, identity.pid);
        let mut listener = transport::LocalControlListener::bind(descriptor.endpoint())
            .map_err(|error| format!("local control endpoint could not bind: {error}"))?;
        publish_endpoint(&state.data_root, &descriptor)?;
        let registry = LocalHostRegistry::current_user();
        let registration = LocalHostRegistration::from_descriptor(
            &state.data_root,
            &descriptor,
            riffra_control::now_ms(),
        );
        if let Err(error) = registry.register(&registration) {
            let _ = remove_endpoint_if_matches(&state.data_root, &instance_id);
            return Err(error);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_instance = instance_id.clone();
        let thread_pid = identity.pid;
        let control_data_root = state.data_root.clone();
        let thread_registry = registry.clone();
        let weak_state = Arc::downgrade(&state);
        let thread = thread::Builder::new()
            .name("riffra-host-control".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok(stream) => {
                            if thread_stop.load(Ordering::Acquire) {
                                break;
                            }
                            let client_state = weak_state.clone();
                            let client_instance = thread_instance.clone();
                            let client_pid = thread_pid;
                            let _ = thread::Builder::new()
                                .name("riffra-host-control-client".into())
                                .spawn(move || {
                                    handle_client(client_state, client_instance, client_pid, stream)
                                });
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
                let _ = thread_registry.unregister(&thread_instance);
            })
            .map_err(|error| {
                let _ = remove_endpoint_if_matches(&control_data_root, &instance_id);
                let _ = registry.unregister(&instance_id);
                format!("local control server could not start: {error}")
            })?;
        tracing::info!(path = %riffra_control::endpoint_path(&control_data_root).display(), "Riffra Host control server is ready");
        Ok(Self {
            stop,
            endpoint: descriptor.endpoint,
            data_root: control_data_root,
            instance_id,
            registry,
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
        let _ = self.registry.unregister(&self.instance_id);
    }
}

fn handle_client(
    state: Weak<HostState>,
    instance_id: String,
    pid: u32,
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
    let event_subscription = if hello.role == ConnectionRole::Events {
        state.upgrade().and_then(|state| state.subscribe_events())
    } else {
        None
    };
    if hello.role == ConnectionRole::Events && event_subscription.is_none() {
        return;
    }
    if transport::write_frame(&mut stream, &HelloResponse::new(instance_id.clone(), pid)).is_err() {
        return;
    }
    match hello.role {
        ConnectionRole::Command => handle_command_client(state, &mut stream),
        ConnectionRole::Events => {
            let Some(subscription) = event_subscription else {
                return;
            };
            handle_event_client(subscription, &mut stream)
        }
    }
}

fn handle_command_client(state: Weak<HostState>, stream: &mut Box<dyn transport::ReadWrite>) {
    loop {
        let frame: Value = match transport::read_frame(&mut **stream) {
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
                    if transport::write_frame(&mut **stream, &response).is_ok() {
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
            Ok(request) => match state.upgrade() {
                Some(state) => state.dispatch_request(request),
                None => riffra_control::ControlResponse::failure(
                    request.request_id,
                    None,
                    ProtocolError::new(ErrorCode::HostUnavailable, "Riffra Host has shut down"),
                ),
            },
            Err(error) => riffra_control::ControlResponse::failure(
                request_id,
                None,
                ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
            ),
        };
        if transport::write_frame(&mut **stream, &response).is_err() {
            return;
        }
    }
}

fn handle_event_client(
    subscription: crate::HostEventSubscription,
    stream: &mut Box<dyn transport::ReadWrite>,
) {
    while let Ok(frame) = subscription.recv() {
        if transport::write_frame(&mut **stream, &frame).is_err() {
            return;
        }
    }
}
