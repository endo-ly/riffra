use std::path::PathBuf;

#[cfg(windows)]
use serde_json::Value;
use tauri::AppHandle;

#[cfg(windows)]
use std::thread;

#[cfg(windows)]
use riffra_control::{
    ControlRequest, ControlResponse, EndpointDescriptor, ErrorCode, HelloRequest, HelloResponse,
    PROTOCOL_VERSION, ProtocolError, endpoint_path, new_instance_id, publish_endpoint,
    remove_endpoint_if_matches, transport,
};

#[cfg(windows)]
use super::router;

#[cfg(windows)]
use crate::AppState;

#[cfg(windows)]
use tauri::Manager;

#[cfg(windows)]
pub(super) fn start(app: AppHandle, data_root: PathBuf) -> Result<(), String> {
    let instance_id = new_instance_id();
    let descriptor = EndpointDescriptor::new(&instance_id, std::process::id());
    let mut listener = transport::NamedPipeListener::bind(&descriptor.pipe_name)
        .map_err(|error| format!("Desktop control pipe could not bind: {error}"))?;
    publish_endpoint(&data_root, &descriptor)?;
    let endpoint = endpoint_path(&data_root);
    let cleanup_data_root = data_root.clone();
    let cleanup_instance_id = instance_id.clone();

    thread::Builder::new()
        .name("riffra-desktop-control".into())
        .spawn(move || {
            loop {
                match listener.accept() {
                    Ok(stream) => {
                        let client_app = app.clone();
                        let client_instance = instance_id.clone();
                        let _ = thread::Builder::new()
                            .name("riffra-desktop-control-client".into())
                            .spawn(move || handle_client(client_app, client_instance, stream));
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Desktop control pipe stopped accepting clients");
                        break;
                    }
                }
            }
            if let Err(error) = remove_endpoint_if_matches(&data_root, &instance_id) {
                tracing::warn!(error = %error, "Desktop control endpoint cleanup failed");
            }
        })
        .map_err(|error| {
            let _ = remove_endpoint_if_matches(&cleanup_data_root, &cleanup_instance_id);
            format!("Desktop control server could not start: {error}")
        })?;
    tracing::info!(path = %endpoint.display(), "Desktop control server is ready");
    Ok(())
}

#[cfg(windows)]
fn handle_client(app: AppHandle, instance_id: String, mut stream: std::fs::File) {
    let hello: HelloRequest = match transport::read_frame(&mut stream) {
        Ok(hello) => hello,
        Err(error) => {
            tracing::debug!(error = %error, "Desktop control client closed during handshake");
            return;
        }
    };
    if hello.message_type != "hello" || hello.protocol_version != PROTOCOL_VERSION {
        let _ = transport::write_frame(
            &mut stream,
            &ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Desktop control handshake was rejected",
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
                let response = ControlResponse::failure(
                    String::new(),
                    None,
                    ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                );
                match &error {
                    transport::TransportError::InvalidUtf8
                    | transport::TransportError::InvalidJson(_) => {
                        if transport::write_frame(&mut stream, &response).is_err() {
                            return;
                        }
                        continue;
                    }
                    transport::TransportError::FrameTooLarge { .. } => {
                        let _ = transport::write_frame(&mut stream, &response);
                    }
                    transport::TransportError::Io(_) => {}
                    transport::TransportError::UnsupportedPlatform => {}
                }
                tracing::debug!(error = %error, "Desktop control client disconnected");
                return;
            }
        };
        let request_id = request_id_from_value(&frame);
        let request: ControlRequest = match serde_json::from_value(frame) {
            Ok(request) => request,
            Err(error) => {
                let response = ControlResponse::failure(
                    request_id,
                    None,
                    ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                );
                if transport::write_frame(&mut stream, &response).is_err() {
                    return;
                }
                continue;
            }
        };
        if let Err(error) = request.validate() {
            let response = ControlResponse::failure(request.request_id, None, error);
            if transport::write_frame(&mut stream, &response).is_err() {
                return;
            }
            continue;
        }
        let response = app
            .try_state::<AppState>()
            .map(|state| router::dispatch(state.inner(), request.clone()))
            .unwrap_or_else(|| {
                ControlResponse::failure(
                    request.request_id,
                    None,
                    ProtocolError::new(
                        ErrorCode::HostUnavailable,
                        "Desktop application state is not ready",
                    ),
                )
            });
        if let Err(error) = transport::write_frame(&mut stream, &response) {
            tracing::debug!(error = %error, "Desktop control response could not be written");
            return;
        }
    }
}

#[cfg(windows)]
fn request_id_from_value(value: &Value) -> String {
    value
        .get("requestId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(not(windows))]
pub(super) fn start(_app: AppHandle, _data_root: PathBuf) -> Result<(), String> {
    Err("Desktop control server requires Windows Named Pipe support".into())
}
