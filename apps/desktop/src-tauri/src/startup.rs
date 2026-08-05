use crate::AppState;
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::{AudioSupervisor, SIDECAR_READY_TIMEOUT};
use crate::rack::application::{self as rack_application, RackContext};
use crate::session::application::{self as session_application, SessionContext};
use crate::session::{CreativeSession, Workspace};

const STARTUP_ATTEMPT_LIMIT: usize = 3;

#[derive(Debug)]
enum StartupError {
    Readiness(String),
    Control(String),
    GenerationChanged(String),
    Fatal(String),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Readiness(message) => {
                write!(
                    formatter,
                    "native audio sidecar readiness failed: {message}"
                )
            }
            Self::Control(message) => {
                write!(formatter, "native audio startup control failed: {message}")
            }
            Self::GenerationChanged(message) => {
                write!(
                    formatter,
                    "native audio sidecar generation changed: {message}"
                )
            }
            Self::Fatal(message) => {
                write!(
                    formatter,
                    "native audio startup state could not be read: {message}"
                )
            }
        }
    }
}

impl std::error::Error for StartupError {}

impl StartupError {
    fn retry_on_same_generation(&self) -> bool {
        matches!(self, Self::Control(_))
    }
}

/// Initializes the audio runtime from the canonical session and releases the
/// startup safety mute only after the current sidecar generation is healthy.
pub(crate) fn initialize_audio_runtime(state: &AppState) -> Result<AudioStatus, String> {
    let _workspace_runtime_gate = state
        .workspace_runtime_gate
        .lock()
        .map_err(|error| format!("Workspace runtime gate was poisoned: {error}"))?;
    let _rack_operation_gate = state
        .rack_operation_gate
        .lock()
        .map_err(|error| format!("Rack operation gate was poisoned: {error}"))?;
    let _session_operation = state.session_actor.enter()?;

    let mut last_error = None;
    for _ in 0..STARTUP_ATTEMPT_LIMIT {
        let generation = state.core.audio().sidecar_generation();
        match initialize_generation(state, generation) {
            Ok(status) if state.core.audio().sidecar_generation() == generation => {
                state.core.audio().mark_startup_complete();
                return Ok(status);
            }
            Ok(_) => continue,
            Err(error) => {
                if state.core.audio().sidecar_generation() != generation {
                    continue;
                }
                if !error.retry_on_same_generation() {
                    state.core.audio().mark_startup_complete();
                    return Err(error.to_string());
                }
                last_error = Some(error);
            }
        }
    }

    state.core.audio().mark_startup_complete();
    Err(last_error.map_or_else(
        || {
            format!(
                "Native audio startup could not complete after {STARTUP_ATTEMPT_LIMIT} sidecar generations."
            )
        },
        |error| error.to_string(),
    ))
}

fn initialize_generation(state: &AppState, generation: u64) -> Result<AudioStatus, StartupError> {
    let audio = state.core.audio();
    audio
        .wait_until_ready(generation, SIDECAR_READY_TIMEOUT)
        .map_err(|error| StartupError::Readiness(error.to_string()))?;

    let session = state
        .core
        .session()
        .lock()
        .map_err(|error| {
            StartupError::Fatal(format!("canonical Session lock was poisoned: {error}"))
        })?
        .clone();

    audio
        .set_master_gain_db(session.settings.master_db)
        .map_err(|error| StartupError::Control(error.to_string()))?;
    ensure_generation(audio, generation)?;

    audio
        .set_processing_mode("passive")
        .map_err(|error| StartupError::Control(error.to_string()))?;
    ensure_generation(audio, generation)?;

    restore_sample_pads(state, &session, generation)?;

    match session.workspace {
        Workspace::Play => restore_play_rack(state, &session, generation)?,
        Workspace::Arrange => sync_arrangement_runtime(state, &session, generation)?,
        Workspace::Home | Workspace::Design => {}
    }

    ensure_generation(audio, generation)?;
    audio
        .set_processing_mode(workspace_processing_mode(session.workspace))
        .map_err(|error| StartupError::Control(error.to_string()))?;
    ensure_generation(audio, generation)?;

    let status = audio
        .refresh_status()
        .map_err(|error| StartupError::Control(error.to_string()))?;
    ensure_generation(audio, generation)?;
    if !safe_to_release_startup_mute(&status) {
        return Ok(status);
    }

    let status = audio
        .set_emergency_mute(false)
        .map_err(|error| StartupError::Control(error.to_string()))?;
    ensure_generation(audio, generation)?;
    Ok(status)
}

fn restore_sample_pads(
    state: &AppState,
    session: &CreativeSession,
    generation: u64,
) -> Result<(), StartupError> {
    let context = SessionContext {
        audio: state.core.audio(),
        runtime: &state.runtime,
        session_actor: &state.session_actor,
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: false,
    };
    if let Err(error) = session_application::restore_sample_pads(&context) {
        if state.core.audio().sidecar_generation() != generation {
            return Err(StartupError::GenerationChanged(generation_changed_message(
                state.core.audio(),
                generation,
            )));
        }
        tracing::warn!(
            workspace = ?session.workspace,
            error = %error,
            "Sample Pad runtime restoration failed during startup."
        );
    }
    ensure_generation(state.core.audio(), generation)
}

fn restore_play_rack(
    state: &AppState,
    session: &CreativeSession,
    generation: u64,
) -> Result<(), StartupError> {
    let context = RackContext {
        audio: state.core.audio(),
        session_actor: &state.session_actor,
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: false,
    };
    if let Err(error) = rack_application::restore_current_rack(&context) {
        if state.core.audio().sidecar_generation() != generation {
            return Err(StartupError::GenerationChanged(generation_changed_message(
                state.core.audio(),
                generation,
            )));
        }
        tracing::warn!(
            workspace = ?session.workspace,
            error = %error,
            "Rack runtime restoration failed during startup."
        );
    }
    ensure_generation(state.core.audio(), generation)
}

fn sync_arrangement_runtime(
    state: &AppState,
    session: &CreativeSession,
    generation: u64,
) -> Result<(), StartupError> {
    let context = SessionContext {
        audio: state.core.audio(),
        runtime: &state.runtime,
        session_actor: &state.session_actor,
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: false,
    };
    if let Err(error) = session_application::sync_arrangement_runtime(&context) {
        if state.core.audio().sidecar_generation() != generation {
            return Err(StartupError::GenerationChanged(generation_changed_message(
                state.core.audio(),
                generation,
            )));
        }
        tracing::warn!(
            workspace = ?session.workspace,
            error = %error,
            "Arrange runtime restoration failed during startup."
        );
    }
    ensure_generation(state.core.audio(), generation)
}

fn generation_changed_message(audio: &AudioSupervisor, expected: u64) -> String {
    format!(
        "Native audio sidecar generation changed from {expected} to {}.",
        audio.sidecar_generation()
    )
}

fn ensure_generation(audio: &AudioSupervisor, expected: u64) -> Result<(), StartupError> {
    let actual = audio.sidecar_generation();
    if actual == expected {
        Ok(())
    } else {
        Err(StartupError::GenerationChanged(generation_changed_message(
            audio, expected,
        )))
    }
}

fn safe_to_release_startup_mute(status: &AudioStatus) -> bool {
    matches!(status.state, AudioState::Ready | AudioState::Muted) && !status.feedback_suspected
}

fn workspace_processing_mode(workspace: Workspace) -> &'static str {
    match workspace {
        Workspace::Play => "play",
        Workspace::Arrange => "arrange",
        Workspace::Home | Workspace::Design => "passive",
    }
}
