use crate::output::compact_agent_response;
use riffra_control::{
    ControlRequest, ControlResponse, ErrorCode, LocalHostClient, LocalHostClientError,
    LocalHostDiscovery, ProtocolError, new_instance_id,
};
use riffra_runtime::api::output::{BackgroundJobStatus, JobState};
use riffra_runtime::api::params::{EmptyParams, IdParams};
use riffra_runtime::api::{
    CommandScope, ControlCommand, ControlOutput, ProjectCommand, RuntimeCommand,
};
use std::thread;
use std::time::{Duration, Instant};

/// A terminal Job response and its success outcome for one-shot CLI output.
pub(super) struct WaitedJob {
    pub(super) response: ControlResponse,
    pub(super) completed: bool,
}

/// Client-only backend for commands owned by a running Riffra Host.
pub struct AttachedBackend {
    client: LocalHostClient,
}

impl AttachedBackend {
    /// Creates an attached backend from a Host selected by the caller.
    pub fn from_discovery(discovery: LocalHostDiscovery) -> Self {
        Self {
            client: discovery.client,
        }
    }

    /// Sends one command and waits for its ordered response.
    ///
    /// A Project-bound command without `expected_project_id` is bound to the
    /// active Project of the Host.
    pub fn request(
        &self,
        request_id: String,
        command: ControlCommand,
        expected_sequence: Option<u64>,
        expected_project_id: Option<String>,
    ) -> Result<ControlResponse, String> {
        let expected_project_id = match (command.policy().scope, expected_project_id) {
            (_, Some(project_id)) => Some(project_id),
            (CommandScope::Project { .. }, None) => Some(self.active_project_id()?),
            (CommandScope::Host, None) => None,
        };
        let mut request = command.into_request(request_id, expected_sequence);
        request.expected_project_id = expected_project_id;
        self.send(&request)
    }

    fn send(&self, request: &ControlRequest) -> Result<ControlResponse, String> {
        self.client.request(request).map_err(|error| match error {
            LocalHostClientError::InvalidRequest(message) => {
                format!("{}: {message}", ErrorCode::InvalidRequest)
            }
            error => format!("{}: {error}", ErrorCode::HostUnavailable),
        })
    }

    /// Polls an existing background job until it reaches a terminal state.
    pub fn wait_for_job(&self, job_id: &str, timeout_ms: Option<u64>) -> Result<WaitedJob, String> {
        let deadline =
            timeout_ms.map(|timeout_ms| Instant::now() + Duration::from_millis(timeout_ms));
        loop {
            let response = self.request(
                format!("cli-job-wait-{}", new_instance_id()),
                RuntimeCommand::JobGet(IdParams {
                    id: job_id.to_owned(),
                })
                .into(),
                None,
                None,
            )?;
            if !response.ok {
                return Ok(WaitedJob {
                    response,
                    completed: false,
                });
            }
            let result = response
                .result
                .clone()
                .ok_or_else(|| "job.get response did not contain a result".to_string())?;
            let state = match ControlOutput::try_from(result) {
                Ok(ControlOutput::Job(Some(
                    BackgroundJobStatus::Scan { state, .. }
                    | BackgroundJobStatus::Render { state, .. }
                    | BackgroundJobStatus::RenderStems { state, .. }
                    | BackgroundJobStatus::AnalysisMeasure { state, .. },
                ))) => state,
                Ok(ControlOutput::Job(None)) => {
                    return Err(format!("background job was not found: {job_id}"));
                }
                _ => return Err("job.get response did not contain a job".into()),
            };
            if matches!(
                state,
                JobState::Cancelled | JobState::Completed | JobState::Failed
            ) {
                return Ok(WaitedJob {
                    response,
                    completed: state == JobState::Completed,
                });
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(format!("timed out waiting for background job: {job_id}"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn active_project_id(&self) -> Result<String, String> {
        let response = self.send(
            &ControlCommand::from(ProjectCommand::ProjectList(EmptyParams {}))
                .into_request(format!("cli-project-state-{}", new_instance_id()), None),
        )?;
        if !response.ok {
            let error = response
                .error
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "Host project state request failed".into());
            return Err(error);
        }
        match response.result.map(ControlOutput::try_from) {
            Some(Ok(ControlOutput::ProjectState(state))) => Ok(state.active_project_id),
            _ => Err("Host project state did not contain activeProjectId".into()),
        }
    }

    /// Forwards JSON Lines from stdin as framed control requests.
    pub fn run_interactive(self) -> Result<(), String> {
        crate::interactive::run(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            |request, command, _| match self.request(
                request.request_id.clone(),
                command,
                request.expected_sequence,
                request.expected_project_id,
            ) {
                Ok(response) => compact_agent_response(response),
                Err(error) => ControlResponse::failure(
                    request.request_id,
                    None,
                    ProtocolError::new(ErrorCode::HostUnavailable, error),
                ),
            },
        )
    }
}
