use super::HostError;
use super::events::{HostEventHub, HostEventSubscription, SharedHostEventSink};
pub use crate::api::output::HostBootstrap;
use crate::audio::AudioSupervisor;
use crate::binaries::RuntimeBinaries;
use crate::instrument::BuiltInInstrumentCatalog;
use crate::jobs::JobRegistry;
use crate::projects;
use crate::render;
use crate::runtime::RuntimeReconciler;
use crate::{AudioPreferences, plugins};
use riffra_control::HostIdentity;
use riffra_core::CanonicalState;
use riffra_host::{DataRootLease, ProjectStore};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub(crate) struct HostState {
    pub(super) _lease: DataRootLease,
    pub(super) identity: HostIdentity,
    pub(crate) data_root: PathBuf,
    pub(crate) project: super::open_project::ProjectCell,
    pub(crate) audio: AudioSupervisor,
    pub(crate) safe_mode: bool,
    pub(super) project_store: ProjectStore,
    pub(crate) runtime: Arc<RuntimeReconciler<AudioSupervisor>>,
    pub(crate) built_in_instruments: Arc<BuiltInInstrumentCatalog>,
    pub(super) events: SharedHostEventSink,
    pub(super) event_hub: Arc<HostEventHub>,
    pub(super) binaries: RuntimeBinaries,
    pub(super) render_worker: render::RenderWorker,
    pub(super) jobs: JobRegistry,
    pub(super) audio_preferences: Mutex<AudioPreferences>,
    pub(super) startup_gate: Mutex<()>,
    pub(super) lifecycle: super::lifecycle::HostLifecycle,
    pub(super) shutdown_requested: AtomicBool,
    pub(super) plugin_persistence_commands:
        Mutex<Option<std::sync::mpsc::Sender<super::persistence::PluginPersistenceCommand>>>,
}

impl HostState {
    pub(super) fn identity(&self) -> &HostIdentity {
        &self.identity
    }

    pub(crate) fn subscribe_events(&self) -> Option<HostEventSubscription> {
        self.event_hub.subscribe()
    }

    pub(super) fn canonical(&self) -> Result<CanonicalState, HostError> {
        Ok(self.project.read().canonical.clone())
    }

    pub(super) fn capture_startup_target(&self) -> Result<crate::startup::StartupTarget, String> {
        let snapshot = self.project.read();
        Ok(crate::startup::StartupTarget {
            project_id: snapshot.canonical.project_id.clone(),
            canonical: riffra_core::CanonicalSnapshot {
                session: snapshot.canonical.session.clone(),
                sequence: snapshot.canonical.sequence,
            },
        })
    }

    pub(super) fn bootstrap(&self) -> Result<HostBootstrap, HostError> {
        let snapshot = self.project.read();
        let recovered_from_generation = snapshot.recovered_from_generation;
        let storage = self
            .project_store
            .session_store(&snapshot.canonical.project_id)
            .map_err(|error| HostError::State(error.to_string()))?;
        Ok(HostBootstrap {
            canonical: snapshot.canonical.clone(),
            project_state: projects::state(&self.project_store, &snapshot.canonical.project_id)
                .map_err(HostError::State)?,
            plugin_catalog: plugins::load(&self.data_root)
                .map_err(|error| HostError::State(error.to_string()))?,
            runtime_started: self.audio.startup_completed(),
            runtime_startup_finished: self.audio.startup_finished(),
            runtime_projection: self.runtime.status(),
            audio_status: self
                .audio
                .status()
                .map_err(|error| HostError::State(error.to_string()))?,
            recovery: projects::recovery(&storage, recovered_from_generation)
                .map_err(HostError::State)?,
            safe_mode: self.safe_mode,
            data_root: self.data_root.clone(),
        })
    }
}

#[cfg(test)]
impl HostState {
    pub(crate) fn for_test(
        data_root: &std::path::Path,
        session: riffra_core::CreativeSession,
        audio: AudioSupervisor,
        safe_mode: bool,
    ) -> Arc<Self> {
        let project_store = ProjectStore::new(data_root);
        let initialized = project_store.initialize().unwrap();
        let lease = DataRootLease::acquire(data_root).unwrap();
        let binaries = RuntimeBinaries::new(
            data_root.join("audio"),
            data_root.join("scan"),
            data_root.join("render"),
            data_root.join("sonalloy"),
        );
        let event_hub = HostEventHub::new(Arc::new(crate::NoopHostEventSink));
        let events: SharedHostEventSink = event_hub.clone();
        let runtime = Arc::new(RuntimeReconciler::new(Arc::new(audio.clone())).unwrap());
        Arc::new(Self {
            _lease: lease,
            identity: HostIdentity::new(),
            data_root: data_root.to_path_buf(),
            project: super::open_project::ProjectCell::new(super::open_project::OpenProject {
                storage: initialized.storage,
                core: riffra_core::AppCore::new(initialized.project_id, session, 0),
                recovered_from_generation: false,
            }),
            audio,
            safe_mode,
            project_store,
            runtime,
            built_in_instruments: Arc::new(crate::test_support::empty_built_in_catalog().clone()),
            events,
            event_hub,
            render_worker: render::RenderWorker::new(binaries.render.clone()),
            binaries,
            jobs: JobRegistry::default(),
            audio_preferences: Mutex::new(AudioPreferences::default()),
            startup_gate: Mutex::new(()),
            lifecycle: super::lifecycle::HostLifecycle::new(),
            shutdown_requested: AtomicBool::new(false),
            plugin_persistence_commands: Mutex::new(None),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{DawHost, HostConfig};

    #[test]
    fn bootstrap_reports_canonical_and_safe_mode_state() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-bootstrap-state-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        let host = DawHost::open(
            HostConfig {
                data_root: data_root.clone(),
                built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                    &data_root,
                ),
                safe_mode: true,
                binaries: RuntimeBinaries::new(
                    data_root.join("riffra-audio"),
                    data_root.join("riffra-plugin-scan"),
                    data_root.join("riffra-render"),
                    data_root.join("sonalloy"),
                ),
            },
            Arc::new(crate::NoopHostEventSink),
        )
        .unwrap();

        let bootstrap = host.state.bootstrap().unwrap();

        assert_eq!(bootstrap.canonical.sequence, 0);
        assert!(bootstrap.safe_mode);
        assert_eq!(bootstrap.audio_status.state, crate::AudioState::Offline);
        assert_eq!(
            bootstrap.runtime_projection.state,
            crate::RuntimeProjectionState::Idle
        );
        assert!(bootstrap.recovery.recovery_candidates.is_empty());

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
