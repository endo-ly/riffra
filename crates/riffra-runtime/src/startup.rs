//! Startup safety transaction for the live native audio Runtime.
//!
//! Startup deliberately keeps the sidecar muted until the selected device is
//! safe, the canonical arrangement has been accepted by the Runtime, and the
//! same sidecar generation is still alive. A failed candidate or a generation
//! change never exposes partially restored audio.

use crate::audio::{
    AudioSupervisor, MuteCause, NativeAudioError, NativeAudioResult, SIDECAR_READY_TIMEOUT,
};
use crate::model::{AudioState, AudioStatus};
use crate::runtime::RuntimeReconciler;
use crate::runtime_snapshot::runtime_timeline_snapshot;
use riffra_core::{AppCore, CanonicalSnapshot};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const STARTUP_SAFETY_TIMEOUT: Duration = Duration::from_secs(45);
const STARTUP_CONTROL_RETRY_LIMIT: usize = 3;
const STARTUP_RUNTIME_GENERATION_RETRY_LIMIT: usize = 3;
const STARTUP_RUNTIME_TARGET_RETRY_LIMIT: usize = 3;
const STARTUP_RUNTIME_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum StartupError {
    Readiness(NativeAudioError),
    Control(NativeAudioError),
    GenerationChanged { expected: u64, actual: u64 },
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

#[derive(Debug)]
enum StartupRuntimeError {
    GenerationChanged(String),
    TargetChanged,
    Feature(String),
    Safety(String),
}

impl std::fmt::Display for StartupRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GenerationChanged(message) | Self::Feature(message) | Self::Safety(message) => {
                formatter.write_str(message)
            }
            Self::TargetChanged => formatter.write_str("startup runtime target changed"),
        }
    }
}

/// Describes the safety result and any non-fatal Runtime restoration failure.
pub(crate) struct StartupInitialization {
    pub(crate) status: AudioStatus,
    pub(crate) runtime_error: Option<String>,
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
    fn refresh_status(&self) -> NativeAudioResult<AudioStatus>;
}

impl StartupAudioPort for AudioSupervisor {
    fn current_generation(&self) -> u64 {
        self.sidecar_generation()
    }

    fn sidecar_terminated(&self, generation: u64) -> bool {
        self.sidecar_terminated(generation)
    }

    fn wait_until_ready(&self, generation: u64, timeout: Duration) -> NativeAudioResult<()> {
        self.wait_until_ready(generation, timeout)
    }

    fn wait_for_next_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<u64> {
        self.wait_for_next_generation(generation, timeout)
    }

    fn set_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<AudioStatus> {
        self.set_master_gain_db(gain_db)
    }

    fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
        self.refresh_status()
    }
}

/// Runs the complete startup transaction for a normal live Host.
pub(crate) fn initialize_runtime(
    core: &AppCore<AudioSupervisor>,
    runtime: &RuntimeReconciler<AudioSupervisor>,
    data_root: &Path,
    shutting_down: &AtomicBool,
) -> Result<StartupInitialization, String> {
    'generation: for _ in 0..STARTUP_RUNTIME_GENERATION_RETRY_LIMIT {
        if shutting_down.load(Ordering::Acquire) {
            core.audio().mark_startup_failed();
            return Err(NativeAudioError::ShuttingDown.to_string());
        }

        let status = match initialize_audio_safety(core) {
            Ok(status) => status,
            Err(error) => {
                core.audio().mark_startup_failed();
                return Err(error);
            }
        };
        if !safe_for_startup_restore(&status) {
            core.audio().mark_startup_failed();
            return Ok(StartupInitialization {
                status,
                runtime_error: Some(
                    "native audio startup safety check failed; feature runtime remains muted"
                        .into(),
                ),
            });
        }

        let generation = core.audio().sidecar_generation();
        for _ in 0..STARTUP_RUNTIME_TARGET_RETRY_LIMIT {
            if shutting_down.load(Ordering::Acquire) {
                core.audio().mark_startup_failed();
                return Err(NativeAudioError::ShuttingDown.to_string());
            }
            match restore_startup_runtime(core, runtime, data_root, generation) {
                Ok(()) => {
                    let status = match release_startup_mute(core.audio(), generation, &status) {
                        Ok(status) => status,
                        Err(StartupRuntimeError::GenerationChanged(_)) => continue 'generation,
                        Err(StartupRuntimeError::TargetChanged) => continue,
                        Err(StartupRuntimeError::Feature(release_error)) => {
                            core.audio().mark_startup_failed();
                            return Ok(StartupInitialization {
                                status,
                                runtime_error: Some(format!(
                                    "Arrangement Runtime restored, but startup safety mute could not be released: {release_error}"
                                )),
                            });
                        }
                        Err(StartupRuntimeError::Safety(error)) => {
                            core.audio().mark_startup_failed();
                            return Ok(StartupInitialization {
                                status,
                                runtime_error: Some(error),
                            });
                        }
                    };
                    if !core.audio().mark_startup_completed(generation) {
                        continue 'generation;
                    }
                    return Ok(StartupInitialization {
                        status,
                        runtime_error: None,
                    });
                }
                Err(StartupRuntimeError::GenerationChanged(_)) => continue 'generation,
                Err(StartupRuntimeError::TargetChanged) => continue,
                Err(StartupRuntimeError::Safety(error)) => {
                    core.audio().mark_startup_failed();
                    return Ok(StartupInitialization {
                        status,
                        runtime_error: Some(error),
                    });
                }
                Err(StartupRuntimeError::Feature(error)) => {
                    match release_startup_mute(core.audio(), generation, &status) {
                        Ok(released) => {
                            core.audio().mark_startup_failed();
                            return Ok(StartupInitialization {
                                status: released,
                                runtime_error: Some(format!(
                                    "{error}; Arrangement Runtime remains muted"
                                )),
                            });
                        }
                        Err(StartupRuntimeError::GenerationChanged(_)) => continue 'generation,
                        Err(StartupRuntimeError::TargetChanged) => continue,
                        Err(StartupRuntimeError::Feature(release_error))
                        | Err(StartupRuntimeError::Safety(release_error)) => {
                            core.audio().mark_startup_failed();
                            return Ok(StartupInitialization {
                                status,
                                runtime_error: Some(format!(
                                    "{error}; startup safety mute could not be released: {release_error}"
                                )),
                            });
                        }
                    }
                }
            }
        }
        core.audio().mark_startup_failed();
        return Ok(StartupInitialization {
            status,
            runtime_error: Some(
                "startup runtime target changed repeatedly; output remains muted".into(),
            ),
        });
    }

    core.audio().mark_startup_failed();
    Err(format!(
        "native audio startup did not converge after {STARTUP_RUNTIME_GENERATION_RETRY_LIMIT} sidecar generations"
    ))
}

fn initialize_audio_safety(core: &AppCore<AudioSupervisor>) -> Result<AudioStatus, String> {
    let master_gain_db = core
        .snapshot()
        .map_err(|error| format!("canonical session could not be captured: {error}"))?
        .session
        .settings
        .master_db;
    initialize_audio_safety_with(core.audio(), master_gain_db, STARTUP_SAFETY_TIMEOUT)
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
    let status = audio.refresh_status().map_err(StartupError::Control)?;
    ensure_generation(audio, generation)?;
    Ok(status)
}

fn restore_startup_runtime(
    core: &AppCore<AudioSupervisor>,
    runtime: &RuntimeReconciler<AudioSupervisor>,
    data_root: &Path,
    generation: u64,
) -> Result<(), StartupRuntimeError> {
    if sidecar_transitioned(core.audio(), generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(core.audio(), generation),
        ));
    }

    let target = core.snapshot().map_err(|error| {
        StartupRuntimeError::Feature(format!("canonical session could not be captured: {error}"))
    })?;
    runtime
        .apply_and_wait(
            runtime_timeline_snapshot(data_root, &target.session),
            riffra_core::ProjectionKey {
                sequence: target.sequence,
                session_revision: target.session.arrangement.revision,
            },
            STARTUP_RUNTIME_TIMEOUT,
        )
        .map_err(|error| {
            StartupRuntimeError::Feature(format!("arrangement runtime restoration failed: {error}"))
        })?;

    if sidecar_transitioned(core.audio(), generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(core.audio(), generation),
        ));
    }
    if !startup_target_is_current(core, &target) {
        tracing::info!("startup runtime target changed during graph restoration");
        return Err(StartupRuntimeError::TargetChanged);
    }
    Ok(())
}

fn release_startup_mute(
    audio: &AudioSupervisor,
    generation: u64,
    muted_status: &AudioStatus,
) -> Result<AudioStatus, StartupRuntimeError> {
    if sidecar_transitioned(audio, generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(audio, generation),
        ));
    }

    let released = audio
        .release_startup_mute_if_allowed(generation)
        .map_err(|error| {
            if sidecar_transitioned(audio, generation) {
                StartupRuntimeError::GenerationChanged(generation_changed_message(
                    audio, generation,
                ))
            } else {
                StartupRuntimeError::Feature(format!(
                    "startup emergency mute could not be released: {error}"
                ))
            }
        })?;

    if sidecar_transitioned(audio, generation) {
        return Err(StartupRuntimeError::GenerationChanged(
            generation_changed_message(audio, generation),
        ));
    }

    if released.is_none()
        && audio.current_mute_cause().map_err(|error| {
            StartupRuntimeError::Safety(format!(
                "startup emergency mute cause could not be read: {error}"
            ))
        })? != Some(MuteCause::User)
    {
        return Err(StartupRuntimeError::Safety(
            "startup emergency mute remains engaged because the audio status is unsafe".into(),
        ));
    }
    Ok(released.unwrap_or_else(|| muted_status.clone()))
}

fn sidecar_transitioned(audio: &AudioSupervisor, generation: u64) -> bool {
    audio.sidecar_generation() != generation || audio.sidecar_terminated(generation)
}

fn startup_target_is_current(core: &AppCore<AudioSupervisor>, target: &CanonicalSnapshot) -> bool {
    core.snapshot()
        .map(|current| current.sequence == target.sequence)
        .unwrap_or(false)
}

fn generation_changed_message(audio: &AudioSupervisor, expected: u64) -> String {
    format!(
        "native audio sidecar generation changed from {expected} to {}",
        audio.sidecar_generation()
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

fn safe_for_startup_restore(status: &AudioStatus) -> bool {
    matches!(status.state, AudioState::Ready | AudioState::Muted) && !status.feedback_suspected
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
        events: Mutex<Vec<String>>,
    }

    impl FakeStartupAudio {
        fn new(status_state: AudioState, lose_first_generation: bool) -> Self {
            Self {
                generation: AtomicU64::new(1),
                status_state,
                lose_first_generation,
                events: Mutex::new(Vec::new()),
            }
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
                midi_inputs: Vec::new(),
                midi_outputs: Vec::new(),
                midi_input_active: false,
                midi_messages: 0,
                last_midi_note: None,
                input_peak: 0.0,
                output_peak: 0.0,
                invalid_samples: 0,
                feedback_suspected: false,
                previewing: false,
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

        fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
            self.record("status");
            Ok(Self::status(self.status_state))
        }
    }

    #[test]
    fn keeps_output_muted_while_retrying_transport_loss_on_a_new_generation() {
        let audio = FakeStartupAudio::new(AudioState::Muted, true);

        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Muted);
        assert_eq!(
            audio.events.lock().unwrap().as_slice(),
            ["lost", "ready", "gain", "status"]
        );
    }

    #[test]
    fn does_not_release_mute_for_a_faulted_status() {
        let audio = FakeStartupAudio::new(AudioState::Faulted, false);

        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Faulted);
        assert!(!safe_for_startup_restore(&status));
    }

    #[test]
    fn retains_a_muted_status_while_preparing_the_runtime() {
        let audio = FakeStartupAudio::new(AudioState::Muted, false);

        let status = initialize_audio_safety_with(&audio, -18.0, Duration::from_secs(1)).unwrap();

        assert_eq!(status.state, AudioState::Muted);
        assert!(safe_for_startup_restore(&status));
    }
}
