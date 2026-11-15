//! Connection to one verified local Riffra Host.

use riffra_control::{
    ControlRequest, ControlResponse, ErrorCode, LocalHostClient, LocalHostClientError,
    LocalHostRegistry, ProtocolError,
};
use riffra_runtime::api::params::EmptyParams;
use riffra_runtime::api::{CommandScope, ControlCommand};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

/// The Host selected for this server process.
pub(crate) struct HostSelection {
    pub(crate) instance_id: String,
    pub(crate) client: LocalHostClient,
}

/// Discovers one running Host and fixes the connection for this process.
///
/// Discovery verifies each registration with `host.status`, so a stale or
/// unrelated endpoint is never selected.
pub(crate) fn connect(requested: Option<&str>) -> Result<HostSelection, String> {
    let registry = LocalHostRegistry::current_user();
    let hosts = registry
        .discover(|registration| {
            ControlRequest::new(
                format!("riffra-mcp-discovery-{}", registration.instance_id),
                "host.status",
                json!({}),
                None,
            )
        })
        .map_err(|error| format!("Riffra Host discovery failed: {error}"))?;
    let selected = select_host(hosts, requested)?;
    Ok(HostSelection {
        instance_id: selected.registration.instance_id,
        client: selected.client,
    })
}

fn select_host(
    mut hosts: Vec<riffra_control::LocalHostDiscovery>,
    requested: Option<&str>,
) -> Result<riffra_control::LocalHostDiscovery, String> {
    hosts.sort_by(|left, right| {
        left.registration
            .instance_id
            .cmp(&right.registration.instance_id)
    });
    let candidates = hosts
        .iter()
        .map(|host| host.registration.instance_id.clone())
        .collect::<Vec<_>>();
    match requested {
        Some(instance_id) => hosts
            .into_iter()
            .find(|host| host.registration.instance_id == instance_id)
            .ok_or_else(|| unavailable_message(Some(instance_id), &candidates)),
        None => match hosts.len() {
            1 => Ok(hosts.remove(0)),
            0 => Err(unavailable_message(None, &[])),
            _ => Err(format!(
                "more than one Riffra Host is running; choose one with --host <instance-id>\nrunning Host instances: {}",
                candidates.join(", ")
            )),
        },
    }
}

fn unavailable_message(requested: Option<&str>, candidates: &[String]) -> String {
    let mut message = match requested {
        Some(instance_id) => format!("Riffra Host {instance_id} is not running"),
        None => "no Riffra Host is running".to_owned(),
    };
    if candidates.is_empty() {
        message.push_str(
            "\nstart Riffra Desktop or `riffra --data-root <path> serve` before starting riffra-mcp",
        );
    } else {
        message.push_str(&format!(
            "\nrunning Host instances: {}",
            candidates.join(", ")
        ));
    }
    message
}

/// Sends Control Commands to the selected Host.
pub(crate) struct HostBridge {
    client: LocalHostClient,
    instance_id: String,
    next_request: AtomicU64,
}

impl HostBridge {
    pub(crate) fn new(selection: HostSelection) -> Self {
        Self {
            client: selection.client,
            instance_id: selection.instance_id,
            next_request: AtomicU64::new(1),
        }
    }

    /// Returns the Host instance this process is fixed to.
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Sends one decoded command, binding Project scope to the active Project
    /// when the caller did not supply an explicit precondition.
    pub(crate) fn request(
        &self,
        command: ControlCommand,
        expected_project_id: Option<String>,
        expected_sequence: Option<u64>,
    ) -> Result<ControlResponse, ProtocolError> {
        let expected_project_id = match (command.policy().scope, expected_project_id) {
            (_, Some(project_id)) => Some(project_id),
            (CommandScope::Project { .. }, None) => Some(self.active_project_id()?),
            (CommandScope::Host, None) => None,
        };
        let command_name = command.name().to_owned();
        let mut request = command.into_request(self.request_id(), expected_sequence);
        request.expected_project_id = expected_project_id;
        tracing::debug!(
            command = command_name,
            request_id = request.request_id,
            "control request"
        );
        self.client.request(&request).map_err(host_error)
    }

    fn active_project_id(&self) -> Result<String, ProtocolError> {
        let mut request = ControlCommand::from(riffra_runtime::api::ProjectCommand::ProjectList(
            EmptyParams {},
        ))
        .into_request(self.request_id(), None);
        request.expected_project_id = None;
        let response = self.client.request(&request).map_err(host_error)?;
        if !response.ok {
            return Err(response.error.unwrap_or_else(|| {
                ProtocolError::new(
                    ErrorCode::HostUnavailable,
                    "Host returned an invalid project.list failure",
                )
            }));
        }
        response
            .result
            .as_ref()
            .and_then(|result| result.value.get("activeProjectId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::HostUnavailable,
                    "Host project.list response did not contain activeProjectId",
                )
            })
    }

    fn request_id(&self) -> String {
        format!(
            "riffra-mcp-{}-{}",
            std::process::id(),
            self.next_request.fetch_add(1, Ordering::Relaxed)
        )
    }
}

fn host_error(error: LocalHostClientError) -> ProtocolError {
    match error {
        LocalHostClientError::InvalidRequest(message) => {
            ProtocolError::new(ErrorCode::InvalidRequest, message)
        }
        other => ProtocolError::new(ErrorCode::HostUnavailable, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_message_names_the_requested_host_and_candidates() {
        // Act
        let message = unavailable_message(Some("host-b"), &["host-a".into(), "host-b".into()]);

        // Assert
        assert!(message.contains("host-b is not running"));
        assert!(message.contains("running Host instances: host-a, host-b"));
    }

    fn discovery(instance_id: &str) -> riffra_control::LocalHostDiscovery {
        let registration = riffra_control::LocalHostRegistration {
            instance_id: instance_id.into(),
            pid: std::process::id(),
            data_root: std::env::temp_dir(),
            endpoint: riffra_control::LocalControlEndpoint::WindowsNamedPipe {
                name: format!("test-{instance_id}"),
            },
            started_at_ms: 0,
        };
        let client = LocalHostClient::connect_registration(&registration);
        riffra_control::LocalHostDiscovery {
            registration,
            client,
        }
    }

    #[test]
    fn host_selection_requires_an_explicit_choice_when_several_are_running() {
        // Act
        let none = select_host(Vec::new(), None).unwrap_err();
        let one = select_host(vec![discovery("host-a")], None).unwrap();
        let several =
            select_host(vec![discovery("host-b"), discovery("host-a")], None).unwrap_err();
        let chosen = select_host(
            vec![discovery("host-b"), discovery("host-a")],
            Some("host-a"),
        )
        .unwrap();
        let missing = select_host(vec![discovery("host-a")], Some("host-b")).unwrap_err();

        // Assert
        assert!(none.contains("no Riffra Host is running"));
        assert_eq!(one.registration.instance_id, "host-a");
        assert!(several.contains("more than one Riffra Host is running"));
        assert!(several.contains("host-a, host-b"));
        assert_eq!(chosen.registration.instance_id, "host-a");
        assert!(missing.contains("host-b is not running"));
    }
}
