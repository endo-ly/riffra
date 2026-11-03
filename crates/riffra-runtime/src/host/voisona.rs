use super::HostState;
use riffra_core::{CanonicalState, EffectOwner};
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const WAIT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Default)]
struct Progress {
    project: String,
    sequence: u64,
    generation: u64,
    complete: bool,
    error: Option<String>,
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
    catalog: (Option<SystemTime>, u64),
}

impl SyncKey {
    fn current(host: &HostState) -> Result<Self, String> {
        let (project, sequence) = host
            .canonical_version()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            project,
            sequence,
            generation: host.runtime.status().runtime_generation,
            catalog: crate::voisona::catalog_stamp(&host.data_root),
        })
    }
}

pub(super) struct Coordinator {
    stop: Arc<AtomicBool>,
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
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new()
            .name("riffra-voisona-sync".into())
            .spawn(move || {
                let mut project = initial.project_id;
                let mut handled: Option<SyncKey> = None;
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
                        thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                    let current = match host.canonical() {
                        Ok(current) => current,
                        Err(error) => {
                            tracing::error!(%error, "VoiSona synchronization stopped");
                            break;
                        }
                    };
                    match synchronize(&host, &current, key.generation, &mut baselines) {
                        Ok(SyncOutcome::Pending | SyncOutcome::Updated) => {}
                        Ok(SyncOutcome::Complete) => {
                            let generation = key.generation;
                            handled = Some(key);
                            status.finish(&current, generation, None);
                        }
                        Err(error) => {
                            tracing::error!(
                                project_id = %project,
                                sequence = current.sequence,
                                %error,
                                "VoiSona synchronization failed"
                            );
                            let generation = key.generation;
                            handled = Some(key);
                            status.finish(&current, generation, Some(error));
                        }
                    }
                    drop(host);
                    thread::sleep(POLL_INTERVAL);
                }
                status.stop();
            })
            .map_err(|error| format!("VoiSona synchronization worker could not start: {error}"))?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn track_baselines(
    data_root: &std::path::Path,
    initial: &CanonicalState,
) -> Result<HashMap<String, CanonicalState>, String> {
    let catalog = crate::voisona::load_catalog(data_root)?;
    Ok(initial
        .session
        .arrangement
        .tracks
        .iter()
        .filter(|track| crate::voisona::is_voisona_track_in(&catalog, track))
        .map(|track| (track.id.clone(), initial.clone()))
        .collect())
}

impl SyncStatus {
    fn finish(&self, current: &CanonicalState, generation: u64, error: Option<String>) {
        let mut progress = self.progress.lock().expect("VoiSona sync lock poisoned");
        progress.project = current.project_id.clone();
        progress.sequence = current.sequence;
        progress.generation = generation;
        progress.complete = true;
        progress.error = error;
        self.changed.notify_all();
    }

    fn stop(&self) {
        let mut progress = self.progress.lock().expect("VoiSona sync lock poisoned");
        progress.stopped = true;
        self.changed.notify_all();
    }

    pub(super) fn wait(
        &self,
        host: &HostState,
        requested: &CanonicalState,
    ) -> Result<CanonicalState, String> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| "VoiSona sync lock poisoned")?;
        loop {
            let (project, sequence) = host
                .canonical_version()
                .map_err(|error| error.to_string())?;
            let generation = host.runtime.status().runtime_generation;
            if project != requested.project_id {
                return Err("project changed while waiting for VoiSona synchronization".into());
            }
            if progress.complete
                && progress.project == project
                && progress.sequence == sequence
                && progress.generation == generation
            {
                let current = host.canonical().map_err(|error| error.to_string())?;
                if current.project_id == project && current.sequence == sequence {
                    if let Some(error) = &progress.error {
                        return Err(error.clone());
                    }
                    return Ok(current);
                }
            }
            if progress.stopped {
                return Err("VoiSona synchronization is unavailable".into());
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
    baselines: &mut HashMap<String, CanonicalState>,
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
            .cloned()
            .unwrap_or_else(|| current.clone());
        let same_device = previous
            .session
            .arrangement
            .tracks
            .iter()
            .find(|candidate| candidate.id == track.id)
            .and_then(|candidate| candidate.instrument.as_ref())
            .is_some_and(|old| {
                old.id == instrument.id
                    && old.as_vst3().map(|plugin| &plugin.path)
                        == instrument.as_vst3().map(|plugin| &plugin.path)
            });
        if !existed || !same_device {
            previous
                .session
                .arrangement
                .midi_clips
                .retain(|clip| clip.track_id != track.id);
        }
        let owner = EffectOwner::Track {
            id: track.id.clone(),
        };
        let before = host
            .audio
            .get_plugin_state(&owner, &instrument.id)
            .map_err(|error| format!("VoiSona track {} ({}): {error}", track.name, track.id))?;
        let Some(encoded) = before.state_data.as_deref() else {
            if crate::voisona::has_notes(&current.session, &track.id)? {
                return Err(format!(
                    "VoiSona track {} ({}): plugin has no configured voice state",
                    track.name, track.id
                ));
            }
            baselines.insert(track.id.clone(), current.clone());
            continue;
        };
        let changed =
            crate::voisona::update_score(encoded, &previous.session, &current.session, &track.id)
                .map_err(|error| format!("VoiSona track {} ({}): {error}", track.name, track.id))?;
        let Some(encoded) = changed else {
            baselines.insert(track.id.clone(), current.clone());
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
                baselines.insert(track.id.clone(), current.clone());
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
