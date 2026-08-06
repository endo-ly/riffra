use crate::AppState;
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::{
    AudioSupervisor, NativeAudioError, NativeAudioResult, SIDECAR_READY_TIMEOUT,
};
use crate::rack::application::{self as rack_application, RackContext};
use crate::session::Workspace;
use crate::session::actor::CanonicalProjection;
use crate::session::application::{self as session_application, SessionContext};
use std::time::{Duration, Instant};

const STARTUP_SAFETY_TIMEOUT: Duration = Duration::from_secs(45);
const STARTUP_CONTROL_RETRY_LIMIT: usize = 3;
const STARTUP_RUNTIME_GENERATION_RETRY_LIMIT: usize = 3;
const STARTUP_RUNTIME_TARGET_RETRY_LIMIT: usize = 3;

#[derive(Debug)]
enum StartupError {
    Readiness(NativeAudioError),
    Control(NativeAudioError),
    GenerationChanged { expected: u64, actual: u64 },
}

#[derive(Debug)]
enum StartupRuntimeError {
    GenerationChanged(String),
    TargetChanged,
    Feature(String),
}

impl std::fmt::Display for StartupRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GenerationChanged(message) | Self::Feature(message) => {
                formatter.write_str(message)
            }
            Self::TargetChanged => formatter.write_str("startup runtime target changed"),
        }
    }
}

/// Describes the safety result and any non-fatal feature-runtime restoration
/// failure observed during startup.
pub(crate) struct StartupInitialization {
    pub(crate) status: AudioStatus,
    pub(crate) runtime_error: Option<String>,
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Readiness(error) => write!(formatter, "native audio readiness failed: {error}"),
            Self::Control(error) => {
                write!(formatter, "native audio startup control failed: {error}")
            }
            Self::GenerationChanged { expected, actual } => write!(
                formatter,
                "native audio sidecar generation changed from {expected} to {actual}"
            ),
        }
    }
}

impl StartupError {
    fn should_wait_for_next_generation<A: StartupAudioPort>(
        &self,
        audio: &A,
        expected_generation: u64,
    ) -> bool {
        if audio.current_generation() != expected_generation {
            return true;
        }
        match self {
            Self::Readiness(error) | Self::Control(error) => {
                matches!(
                    error,
                    NativeAudioError::TransportLost { .. }
                        | NativeAudioError::GenerationChanged { .. }
                ) || audio.sidecar_terminated(expected_generation)
            }
            Self::GenerationChanged { .. } => true,
        }
    }

    fn retry_same_generation(&self) -> bool {
        matches!(
            self,
            Self::Control(NativeAudioError::Timeout { .. })
                | Self::Control(NativeAudioError::DeadlineExpired)
        )
    }
}

trait StartupAudioPort {
    fn current_generation(&self) -> u64;
    fn sidecar_terminated(&self, generation: u64) -> bool;
    fn wait_until_ready(&self, generation: u64, timeout: Duration) -> NativeAudioResult<()>;
    fn wait_for_next_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<u64>;
    fn set_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<AudioStatus>;
    fn set_processing_mode(&self, mode: &str) -> NativeAudioResult<AudioStatus>;
    fn refresh_status(&self) -> NativeAudioResult<AudioStatus>;
    fn startup_unmute_allowed(&self) -> NativeAudioResult<bool>;
    fn set_emergency_mute(&self, muted: bool) -> NativeAudioResult<AudioStatus>;
}

impl StartupAudioPort for AudioSupervisor {
    fn current_generation(&self) -> u64 {
        self.sidecar_generation()
    }

    fn sidecar_terminated(&self, generation: u64) -> bool {
        AudioSupervisor::sidecar_terminated(self, generation)
    }

    fn wait_until_ready(&self, generation: u64, timeout: Duration) -> NativeAudioResult<()> {
        AudioSupervisor::wait_until_ready(self, generation, timeout)
    }

    fn wait_for_next_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<u64> {
        AudioSupervisor::wait_for_next_generation(self, generation, timeout)
    }

    fn set_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<AudioStatus> {
        AudioSupervisor::set_master_gain_db(self, gain_db)
    }

    fn set_processing_mode(&self, mode: &str) -> NativeAudioResult<AudioStatus> {
        AudioSupervisor::set_processing_mode(self, mode)
    }

    fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
        AudioSupervisor::refresh_status(self)
    }

    fn startup_unmute_allowed(&self) -> NativeAudioResult<bool> {
        AudioSupervisor::startup_unmute_allowed(self)
    }

    fn set_emergency_mute(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
        AudioSupervisor::set_emergency_mute(self, muted)
    }
}

/// Waits for the current sidecar to become safe and releases only the startup
/// safety mute. Feature runtime restoration is intentionally performed later,
/// while the application remains in passive processing mode.
pub(crate) fn initialize_audio_safety(state: &AppState) -> Result<AudioStatus, String> {
    let master_gain_db = state
        .session_actor
        .capture_projection(state.core.session())
        .map_err(|error| format!("canonical session could not be captured: {error}"))?
        .session
        .settings
        .master_db;
    initialize_audio_safety_with(state.core.audio(), master_gain_db, STARTUP_SAFETY_TIMEOUT)
}

/// Completes the startup safety transaction and then reconciles the initial
/// feature runtime without holding application operation gates. A sidecar
/// restart during feature restoration re-enters the safety transaction on the
/// replacement generation before startup is marked complete.
pub(crate) fn initialize_audio_runtime<F>(
    state: &AppState,
    on_safety_boundary: F,
) -> Result<StartupInitialization, String>
where
    F: FnOnce(),
{
    let mut safety_boundary_notified = Some(on_safety_boundary);
    'generation: for _ in 0..STARTUP_RUNTIME_GENERATION_RETRY_LIMIT {
        let safety_result = initialize_audio_safety(state);
        if let Some(notify) = safety_boundary_notified.take() {
            notify();
        }
        let status = match safety_result {
            Ok(status) => status,
            Err(error) => {
                state.core.audio().mark_startup_failed();
                return Err(error);
            }
        };
        if !safe_to_release_startup_mute(&status) {
            state.core.audio().mark_startup_failed();
            return Ok(StartupInitialization {
                status,
                runtime_error: Some(
                    "native audio startup safety check failed; feature runtime remains passive"
                        .into(),
                ),
            });
        }
        let generation = state.core.audio().sidecar_generation();
        for _ in 0..STARTUP_RUNTIME_TARGET_RETRY_LIMIT {
            match restore_startup_runtime(state, generation) {
                Ok(()) => {
                    if !state.core.audio().mark_startup_completed(generation) {
                        continue 'generation;
                    }
                    return Ok(StartupInitialization {
                        status,
                        runtime_error: None,
                    });
                }
                Err(StartupRuntimeError::GenerationChanged(_)) => continue 'generation,
                Err(StartupRuntimeError::TargetChanged) => continue,
                Err(StartupRuntimeError::Feature(error)) => {
                    if !state.core.audio().mark_startup_completed(generation) {
                        continue 'generation;
                    }
                    return Ok(StartupInitialization {
                        status,
                        runtime_error: Some(error),
                    });
                }
            }
        }
        if !state.core.audio().mark_startup_completed(generation) {
            continue 'generation;
        }
        return Ok(StartupInitialization {
            status,
            runtime_error: Some(
                "startup runtime target changed repeatedly; passive mode remains active".into(),
            ),
        });
    }

    state.core.audio().mark_startup_failed();
    Err(format!(
        "native audio startup did not converge after {STARTUP_RUNTIME_GENERATION_RETRY_LIMIT} sidecar generations"
    ))
}

fn initialize_audio_safety_with<A: StartupAudioPort>(
    audio: &A,
    master_gain_db: f64,
    timeout: Duration,
) -> Result<AudioStatus, String> {
    let deadline = Instant::now() + timeout;
    let mut control_retries = 0;
    let mut retry_generation = None;

    loop {
        let generation = audio.current_generation();
        if retry_generation != Some(generation) {
            retry_generation = Some(generation);
            control_retries = 0;
        }
        match initialize_safety_generation(audio, generation, master_gain_db) {
            Ok(status) if audio.current_generation() == generation => return Ok(status),
            Ok(_) => continue,
            Err(error) => {
                if error.should_wait_for_next_generation(audio, generation) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(error.to_string());
                    }
                    if audio
                        .wait_for_next_generation(generation, remaining)
                        .is_ok()
                    {
                        continue;
                    }
                    return Err(error.to_string());
                }
                if error.retry_same_generation() && control_retries < STARTUP_CONTROL_RETRY_LIMIT {
                    control_retries += 1;
                    continue;
                }
                return Err(error.to_string());
            }
        }
    }
}

fn initialize_safety_generation<A: StartupAudioPort>(
    audio: &A,
    generation: u64,
    master_gain_db: f64,
) -> Result<AudioStatus, StartupError> {
    audio
        .wait_until_ready(generation, SIDECAR_READY_TIMEOUT)
        .map_err(StartupError::Readiness)?;

    audio
        .set_master_gain_db(master_gain_db)
        .map_err(StartupError::Control)?;
    ensure_generation(audio, generation)?;

    audio
        .set_processing_mode("passive")
        .map_err(StartupError::Control)?;
    ensure_generation(audio, generation)?;

    let status = audio.refresh_status().map_err(StartupError::Control)?;
    ensure_generation(audio, generation)?;
    if !safe_to_release_startup_mute(&status)
        || !audio
            .startup_unmute_allowed()
            .map_err(StartupError::Control)?
    {
        return Ok(status);
    }

    let status = audio
        .set_emergency_mute(false)
        .map_err(StartupError::Control)?;
    ensure_generation(audio, generation)?;
    Ok(status)
}

/// Restores feature-specific runtime state after the safety mute has already
/// been released. No long-running VST or timeline operation owns a Session,
/// Rack, or Workspace gate. If the canonical target changes while restoration
/// is running, the old target is left passive and the newer request wins.
fn restore_startup_runtime(state: &AppState, generation: u64) -> Result<(), StartupRuntimeError> {
    if state.core.safe_mode() {
        return Ok(());
    }

    if sidecar_transitioned(state, generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(state.core.audio(), generation),
        ));
    }

    let target = state
        .session_actor
        .capture_projection(state.core.session())
        .map_err(|error| {
            StartupRuntimeError::Feature(format!(
                "canonical session could not be captured: {error}"
            ))
        })?;
    let mut failures = Vec::new();

    let session_context = SessionContext {
        audio: state.core.audio(),
        runtime: &state.runtime,
        session_actor: &state.session_actor,
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: false,
    };
    if let Err(error) = session_application::restore_sample_pads(&session_context) {
        failures.push(format!("sample pad restoration failed: {error}"));
    }
    if sidecar_transitioned(state, generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(state.core.audio(), generation),
        ));
    }
    if !startup_target_is_current(state, &target) {
        tracing::info!("startup runtime target changed before workspace restoration");
        return Err(StartupRuntimeError::TargetChanged);
    }

    match target.session.workspace {
        Workspace::Play => {
            let context = RackContext {
                audio: state.core.audio(),
                session_actor: &state.session_actor,
                data_root: state.core.data_root(),
                session: state.core.session(),
                safe_mode: false,
            };
            if let Err(error) = rack_application::restore_current_rack(&context) {
                failures.push(format!("rack restoration failed: {error}"));
            }
        }
        Workspace::Arrange => {
            if let Err(error) = session_application::sync_arrangement_runtime(&session_context) {
                failures.push(format!("arrange runtime restoration failed: {error}"));
            }
        }
        Workspace::Home | Workspace::Design => {}
    }

    if sidecar_transitioned(state, generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(state.core.audio(), generation),
        ));
    }
    if !startup_target_is_current(state, &target) {
        tracing::info!("startup runtime target changed during workspace restoration");
        return Err(StartupRuntimeError::TargetChanged);
    }

    let mode = workspace_processing_mode(target.session.workspace);
    if failures.is_empty() {
        let mode_error = state
            .core
            .audio()
            .set_processing_mode_nonblocking(mode)
            .err();
        if let Some(error) = mode_error {
            if sidecar_transitioned(state, generation) {
                return Err(StartupRuntimeError::GenerationChanged(
                    generation_changed_message(state.core.audio(), generation),
                ));
            }
            let passive_error = state
                .core
                .audio()
                .set_processing_mode_nonblocking("passive")
                .err();
            let message = match passive_error {
                Some(passive_error) => format!(
                    "startup processing mode could not be applied: {error}; passive mode could not be maintained: {passive_error}"
                ),
                None => format!("startup processing mode could not be applied: {error}"),
            };
            return Err(StartupRuntimeError::Feature(message));
        }
        if sidecar_transitioned(state, generation) {
            return Err(StartupRuntimeError::GenerationChanged(
                generation_changed_message(state.core.audio(), generation),
            ));
        }
        Ok(())
    } else {
        state
            .core
            .audio()
            .set_processing_mode_nonblocking("passive")
            .map_err(|error| {
                if sidecar_transitioned(state, generation) {
                    StartupRuntimeError::GenerationChanged(generation_changed_message(
                        state.core.audio(),
                        generation,
                    ))
                } else {
                    StartupRuntimeError::Feature(format!(
                        "startup passive mode could not be maintained: {error}"
                    ))
                }
            })?;
        Err(StartupRuntimeError::Feature(failures.join("; ")))
    }
}

fn sidecar_transitioned(state: &AppState, generation: u64) -> bool {
    state.core.audio().sidecar_generation() != generation
        || state.core.audio().sidecar_terminated(generation)
}

fn startup_target_is_current(state: &AppState, target: &CanonicalProjection) -> bool {
    state
        .session_actor
        .capture_projection(state.core.session())
        .map(|current| {
            current.sequence == target.sequence
                && current.session.workspace == target.session.workspace
        })
        .unwrap_or(false)
}

fn generation_changed_message<A: StartupAudioPort>(audio: &A, expected: u64) -> String {
    format!(
        "native audio sidecar generation changed from {expected} to {}",
        audio.current_generation()
    )
}

fn ensure_generation<A: StartupAudioPort>(audio: &A, expected: u64) -> Result<(), StartupError> {
    let actual = audio.current_generation();
    if actual == expected {
        Ok(())
    } else {
        Err(StartupError::GenerationChanged { expected, actual })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RecordingStatus;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeStartupAudio {
        generation: AtomicU64,
        status_state: AudioState,
        lose_first_generation: bool,
        allow_startup_unmute: bool,
        unmute_calls: Mutex<Vec<u64>>,
        events: Mutex<Vec<String>>,
    }

    impl FakeStartupAudio {
        fn new(status_state: AudioState, lose_first_generation: bool) -> Self {
            Self {
                generation: AtomicU64::new(1),
                status_state,
                lose_first_generation,
                allow_startup_unmute: true,
                unmute_calls: Mutex::new(Vec::new()),
                events: Mutex::new(Vec::new()),
            }
        }

        fn with_startup_unmute_allowed(mut self, allowed: bool) -> Self {
            self.allow_startup_unmute = allowed;
            self
        }

        fn status(state: AudioState) -> AudioStatus {
            AudioStatus {
                state,
                driver: Some("fake".into()),
                input_device: None,
                input_channel: None,
                input_channels: Vec::new(),
                output_device: None,
                output_channels: Vec::new(),
                sample_rate: None,
                buffer_size: None,
                round_trip_ms: None,
                timeline_tick: None,
                recording: RecordingStatus::default(),
                plugin: None,
                midi_inputs: Vec::new(),
                midi_outputs: Vec::new(),
                midi_input_active: false,
                midi_messages: 0,
                last_midi_note: None,
                midi_pad_mappings: 0,
                midi_pad_triggers: 0,
                input_peak: 0.0,
                output_peak: 0.0,
                invalid_samples: 0,
                feedback_suspected: false,
                message: "fake".into(),
            }
        }

        fn record(&self, event: &str) {
            self.events.lock().unwrap().push(event.into());
        }
    }

    impl StartupAudioPort for FakeStartupAudio {
        fn current_generation(&self) -> u64 {
            self.generation.load(Ordering::Acquire)
        }

        fn sidecar_terminated(&self, _generation: u64) -> bool {
            false
        }

        fn wait_until_ready(&self, generation: u64, _timeout: Duration) -> NativeAudioResult<()> {
            self.record(if generation == 1 { "lost" } else { "ready" });
            if self.lose_first_generation && generation == 1 {
                Err(NativeAudioError::transport_lost("fake sidecar stopped"))
            } else {
                Ok(())
            }
        }

        fn wait_for_next_generation(
            &self,
            _generation: u64,
            _timeout: Duration,
        ) -> NativeAudioResult<u64> {
            self.generation.store(2, Ordering::Release);
            Ok(2)
        }

        fn set_master_gain_db(&self, _gain_db: f64) -> NativeAudioResult<AudioStatus> {
            self.record("gain");
            Ok(Self::status(AudioState::Muted))
        }

        fn set_processing_mode(&self, _mode: &str) -> NativeAudioResult<AudioStatus> {
            self.record("passive");
            Ok(Self::status(AudioState::Muted))
        }

        fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
            self.record("status");
            Ok(Self::status(self.status_state))
        }

        fn startup_unmute_allowed(&self) -> NativeAudioResult<bool> {
            Ok(self.allow_startup_unmute)
        }

        fn set_emergency_mute(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
            if !muted {
                self.unmute_calls
                    .lock()
                    .unwrap()
                    .push(self.current_generation());
            }
            Ok(Self::status(if muted {
                AudioState::Muted
            } else {
                AudioState::Ready
            }))
        }
    }

    #[test]
    fn retries_a_transport_loss_on_the_new_sidecar_generation() {
        let audio = FakeStartupAudio::new(AudioState::Muted, true);

        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Ready);
        assert_eq!(*audio.unmute_calls.lock().unwrap(), vec![2]);
        assert_eq!(
            audio.events.lock().unwrap().as_slice(),
            ["lost", "ready", "gain", "passive", "status"]
        );
    }

    #[test]
    fn does_not_release_mute_for_a_faulted_status() {
        let audio = FakeStartupAudio::new(AudioState::Faulted, false);
        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Faulted);
        assert!(audio.unmute_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn does_not_release_a_manual_startup_mute() {
        let audio =
            FakeStartupAudio::new(AudioState::Muted, false).with_startup_unmute_allowed(false);

        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Muted);
        assert!(audio.unmute_calls.lock().unwrap().is_empty());
    }
}
