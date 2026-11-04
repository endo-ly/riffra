use super::HostState;
use crate::voisona::{SyncHistory, SyncedPluginState, TrackScore};
use riffra_core::{CanonicalState, EffectOwner};
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const WAIT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Default)]
struct Progress {
    key: Option<SyncKey>,
    error: Option<String>,
    changes: SyncHistory,
    stopped: bool,
}

#[derive(Default)]
pub(super) struct SyncStatus {
    progress: Mutex<Progress>,
    changed: Condvar,
}

#[derive(Clone, PartialEq, Eq)]
struct SyncKey {
    project: String,
    sequence: u64,
    generation: u64,
    catalog: u64,
    projection_state: crate::api::output::RuntimeProjectionState,
    projection_operation: u64,
    audio_environment_revision: u64,
}

impl SyncKey {
    fn current(host: &HostState) -> Result<Self, String> {
        let (project, sequence) = host
            .canonical_version()
            .map_err(|error| error.to_string())?;
        let projection = host.runtime.status();
        Ok(Self {
            project,
            sequence,
            generation: projection.runtime_generation,
            catalog: host.event_hub.catalog_generation(),
            projection_state: projection.state,
            projection_operation: projection.operation_id,
            audio_environment_revision: projection.audio_environment_revision,
        })
    }
}

pub(super) struct Coordinator {
    stop: Arc<AtomicBool>,
    wake: std::sync::mpsc::SyncSender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Coordinator {
    pub(super) fn start(
        host: Weak<HostState>,
        status: Arc<SyncStatus>,
        initial: CanonicalState,
        data_root: &std::path::Path,
    ) -> Result<Self, String> {
        let mut baselines = track_baselines(data_root, &initial)?;
        let (wake, notifications) = host
            .upgrade()
            .ok_or("Host is unavailable")?
            .event_hub
            .subscribe_voisona()
            .ok_or("Host event hub is closed")?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new()
            .name("riffra-voisona-sync".into())
            .spawn(move || {
                let mut project = initial.project_id;
                let mut handled: Option<SyncKey> = None;
                let mut changes = SyncHistory::default();
                while !stopping.load(Ordering::Acquire) {
                    let Some(host) = host.upgrade() else { break };
                    let key = match SyncKey::current(&host) {
                        Ok(key) => key,
                        Err(error) => {
                            tracing::error!(%error, "VoiSona synchronization stopped");
                            break;
                        }
                    };
                    if key.project != project {
                        let current = match host.canonical() {
                            Ok(current) => current,
                            Err(error) => {
                                tracing::error!(%error, "VoiSona synchronization stopped");
                                break;
                            }
                        };
                        project = key.project.clone();
                        changes.clear();
                        baselines = match track_baselines(&host.data_root, &current) {
                            Ok(baselines) => baselines,
                            Err(error) => {
                                tracing::error!(%error, "VoiSona synchronization stopped");
                                break;
                            }
                        };
                        handled = None;
                    }
                    if handled.as_ref() == Some(&key) {
                        drop(host);
                        if notifications.recv().is_err() {
                            break;
                        }
                        while notifications.try_recv().is_ok() {}
                        continue;
                    }
                    let current = match host.canonical() {
                        Ok(current) => current,
                        Err(error) => {
                            tracing::error!(%error, "VoiSona synchronization stopped");
                            break;
                        }
                    };
                    if current.project_id != key.project || current.sequence != key.sequence {
                        continue;
                    }
                    match synchronize(
                        &host,
                        &current,
                        key.generation,
                        &mut baselines,
                        &mut changes,
                    ) {
                        Ok(SyncOutcome::Pending | SyncOutcome::Updated) => {}
                        Ok(SyncOutcome::Complete) => {
                            handled = Some(key.clone());
                            status.finish(key, None, std::mem::take(&mut changes));
                        }
                        Err(error) => {
                            tracing::error!(
                                project_id = %project,
                                sequence = current.sequence,
                                %error,
                                "VoiSona synchronization failed"
                            );
                            handled = Some(key.clone());
                            changes.clear();
                            status.finish(key, Some(error), SyncHistory::default());
                        }
                    }
                    drop(host);
                    if notifications.recv().is_err() {
                        break;
                    }
                    while notifications.try_recv().is_ok() {}
                }
                status.stop();
            })
            .map_err(|error| format!("VoiSona synchronization worker could not start: {error}"))?;
        Ok(Self {
            stop,
            wake,
            worker: Some(worker),
        })
    }

    pub(super) fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct TrackBaseline {
    instrument_id: String,
    plugin_path: String,
    score: TrackScore,
}

impl TrackBaseline {
    fn from_session(session: &riffra_core::CreativeSession, track: &riffra_core::Track) -> Self {
        let instrument = track
            .instrument
            .as_ref()
            .expect("VoiSona track has an instrument");
        let plugin = instrument
            .as_vst3()
            .expect("VoiSona track has a VST3 instrument");
        Self {
            instrument_id: instrument.id.clone(),
            plugin_path: plugin.path.clone(),
            score: TrackScore::from_session(session, &track.id),
        }
    }
}

fn track_baselines(
    data_root: &std::path::Path,
    initial: &CanonicalState,
) -> Result<HashMap<String, TrackBaseline>, String> {
    let catalog = crate::voisona::load_catalog(data_root)?;
    Ok(initial
        .session
        .arrangement
        .tracks
        .iter()
        .filter(|track| crate::voisona::is_voisona_track_in(&catalog, track))
        .map(|track| {
            (
                track.id.clone(),
                TrackBaseline::from_session(&initial.session, track),
            )
        })
        .collect())
}

pub(super) struct WaitResult {
    pub(super) canonical: CanonicalState,
    pub(super) changes: SyncHistory,
}

impl SyncStatus {
    fn finish(&self, key: SyncKey, error: Option<String>, changes: SyncHistory) {
        let mut progress = self.progress.lock().expect("VoiSona sync lock poisoned");
        progress.key = Some(key);
        progress.error = error;
        progress.changes = changes;
        self.changed.notify_all();
    }

    fn stop(&self) {
        let mut progress = self.progress.lock().expect("VoiSona sync lock poisoned");
        progress.stopped = true;
        self.changed.notify_all();
    }

    pub(super) fn wake_waiters(&self) {
        let _progress = self.progress.lock().expect("VoiSona sync lock poisoned");
        self.changed.notify_all();
    }

    pub(super) fn wait(
        &self,
        host: &HostState,
        requested: &CanonicalState,
        generation_at_request: u64,
        cancelled: Option<&AtomicBool>,
        play_operation: Option<crate::runtime::TransportOperationId>,
    ) -> Result<WaitResult, String> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| "VoiSona sync lock poisoned")?;
        loop {
            if progress.stopped {
                return Err("VoiSona synchronization is unavailable".into());
            }
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                return Err("VoiSona synchronization was cancelled".into());
            }
            if let Some(operation) = play_operation
                && !host
                    .runtime
                    .sync_play_is_current(operation)
                    .map_err(|error| error.to_string())?
            {
                return Err("VoiSona synchronization was cancelled".into());
            }
            let key = SyncKey::current(host)?;
            let project = &key.project;
            let sequence = key.sequence;
            let generation = key.generation;
            if generation != generation_at_request {
                return Err("runtime changed while waiting for VoiSona synchronization".into());
            }
            if *project != requested.project_id {
                return Err("project changed while waiting for VoiSona synchronization".into());
            }
            if progress.key.as_ref() == Some(&key) {
                let current = host.canonical().map_err(|error| error.to_string())?;
                if current.project_id == *project && current.sequence == sequence {
                    if let Some(error) = &progress.error {
                        return Err(error.clone());
                    }
                    return Ok(WaitResult {
                        canonical: current,
                        changes: progress.changes.clone(),
                    });
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "VoiSona synchronization did not complete within {WAIT_TIMEOUT:?}"
                ));
            }
            progress = self
                .changed
                .wait_timeout(progress, remaining.min(Duration::from_millis(100)))
                .map_err(|_| "VoiSona sync lock poisoned")?
                .0;
        }
    }
}

enum SyncOutcome {
    Pending,
    Updated,
    Complete,
}

fn synchronize(
    host: &HostState,
    current: &CanonicalState,
    generation: u64,
    baselines: &mut HashMap<String, TrackBaseline>,
    changes: &mut SyncHistory,
) -> Result<SyncOutcome, String> {
    let catalog = crate::voisona::load_catalog(&host.data_root)?;
    let targets = current
        .session
        .arrangement
        .tracks
        .iter()
        .filter(|track| crate::voisona::is_target_in(&catalog, track))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(SyncOutcome::Complete);
    }
    if host.safe_mode {
        return Err("VoiSona synchronization requires the audio runtime".into());
    }
    let projection = host.runtime.status();
    if projection.runtime_generation != generation {
        return Ok(SyncOutcome::Pending);
    }
    if projection.active_projection_sequence != Some(current.sequence)
        || projection.active_session_revision != Some(current.session.arrangement.revision)
    {
        if projection.state == crate::api::output::RuntimeProjectionState::Failed {
            return Err(projection.last_error.unwrap_or_else(|| {
                "audio projection failed before VoiSona synchronization".into()
            }));
        }
        return Ok(SyncOutcome::Pending);
    }
    for track in targets {
        let instrument = track
            .instrument
            .as_ref()
            .expect("VoiSona instrument exists");
        let existed = baselines.contains_key(&track.id);
        let mut previous = baselines
            .get(&track.id)
            .map(|baseline| baseline.score.clone())
            .unwrap_or_else(|| TrackScore::from_session(&current.session, &track.id));
        let same_device = baselines.get(&track.id).is_some_and(|baseline| {
            baseline.instrument_id == instrument.id
                && instrument
                    .as_vst3()
                    .is_some_and(|plugin| baseline.plugin_path == plugin.path)
        });
        if !existed || !same_device {
            previous.clear_clips();
        }
        let owner = EffectOwner::Track {
            id: track.id.clone(),
        };
        let before = host
            .audio
            .get_plugin_state(&owner, &instrument.id)
            .map_err(|error| format!("VoiSona track {} ({}): {error}", track.name, track.id))?;
        let Some(encoded) = before.state_data.as_deref() else {
            if crate::voisona::has_sounding_notes(&current.session, &track.id)? {
                return Err(format!(
                    "VoiSona track {} ({}): plugin has no configured voice state",
                    track.name, track.id
                ));
            }
            baselines.insert(
                track.id.clone(),
                TrackBaseline::from_session(&current.session, track),
            );
            continue;
        };
        let current_score = TrackScore::from_session(&current.session, &track.id);
        let changed = crate::voisona::update_score(encoded, &previous, &current_score)
            .map_err(|error| format!("VoiSona track {} ({}): {error}", track.name, track.id))?;
        let Some(encoded) = changed else {
            baselines.insert(
                track.id.clone(),
                TrackBaseline::from_session(&current.session, track),
            );
            continue;
        };
        let mut writer = host.project.write();
        let latest = writer.project().core.canonical_state();
        if latest.project_id != current.project_id || latest.sequence != current.sequence {
            return Ok(SyncOutcome::Pending);
        }
        let live = host.runtime.status();
        if live.runtime_generation != projection.runtime_generation
            || live.active_projection_sequence != Some(current.sequence)
            || live.active_session_revision != Some(current.session.arrangement.revision)
        {
            return Ok(SyncOutcome::Pending);
        }
        let mut desired = before.clone();
        desired.state_data = Some(encoded);
        host.audio
            .compare_and_set_plugin_state(&owner, &instrument.id, before.clone(), desired)
            .map_err(|error| format!("VoiSona track {} ({}): {error}", track.name, track.id))?;
        let applied = host
            .audio
            .get_plugin_state(&owner, &instrument.id)
            .map_err(|error| error.to_string())?;
        let persisted = writer.commit(|mut app| {
            app.persist_plugin_state(
                &owner,
                &instrument.id,
                applied.parameter_values.clone(),
                applied.state_data.clone(),
                applied.bypassed,
            )
        });
        match persisted {
            Ok(committed) => {
                changes.record(SyncedPluginState {
                    owner: owner.clone(),
                    device_id: instrument.id.clone(),
                    before: before.clone(),
                    after: applied.clone(),
                });
                baselines.insert(
                    track.id.clone(),
                    TrackBaseline::from_session(&current.session, track),
                );
                host.publish_commit(committed);
                return Ok(SyncOutcome::Updated);
            }
            Err(error) => {
                host.audio
                    .compare_and_set_plugin_state(&owner, &instrument.id, applied, before)
                    .map_err(|rollback| format!("{error}; VoiSona rollback failed: {rollback}"))?;
                return Err(error.to_string());
            }
        }
    }
    Ok(SyncOutcome::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_synchronization_can_be_cancelled() {
        let root = std::env::temp_dir().join(format!(
            "riffra-voisona-wait-{}",
            riffra_control::new_instance_id()
        ));
        let host = HostState::for_test(
            &root,
            riffra_core::CreativeSession::new(1),
            crate::AudioSupervisor::offline("test"),
            true,
        );
        let requested = host.canonical().unwrap();
        let generation = host.runtime.status().runtime_generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        let status = Arc::clone(&host.voisona_status);
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker_host = Arc::clone(&host);
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_status = Arc::clone(&status);
        let worker = thread::spawn(move || {
            sender.send(()).unwrap();
            worker_status.wait(
                &worker_host,
                &requested,
                generation,
                Some(&worker_cancelled),
                None,
            )
        });
        receiver.recv().unwrap();
        cancelled.store(true, Ordering::Release);
        status.wake_waiters();
        assert!(matches!(worker.join().unwrap(), Err(error) if error.contains("cancelled")));
        let requested = host.canonical().unwrap();
        let key = SyncKey::current(&host).unwrap();
        status.finish(
            key.clone(),
            Some("voice synchronization failed".into()),
            SyncHistory::default(),
        );
        assert!(
            matches!(status.wait(&host, &requested, generation, None, None), Err(error) if error == "voice synchronization failed")
        );
        status.finish(key, None, SyncHistory::default());
        status.stop();
        assert!(
            matches!(status.wait(&host, &requested, generation, None, None), Err(error) if error.contains("unavailable"))
        );
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }
}
