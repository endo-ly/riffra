use super::HostError;
use super::events::{HostEventHub, HostEventSubscription, SharedHostEventSink};
use crate::audio::AudioSupervisor;
use crate::binaries::RuntimeBinaries;
use crate::instrument::{BuiltInInstrumentCatalog, BuiltInInstrumentSummary};
use crate::jobs::JobRegistry;
use crate::model::{AudioStatus, ProjectRecoveryState, ProjectState, RuntimeProjectionStatus};
use crate::projects;
use crate::render;
use crate::runtime::RuntimeReconciler;
use crate::{AudioPreferences, plugins};
use riffra_control::HostIdentity;
use riffra_core::{AppCore, CanonicalState};
use riffra_host::{DataRootLease, ProjectStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

/// Host-owned state required to initialize an embedded or attached Desktop.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBootstrap {
    pub canonical: CanonicalState,
    pub project_state: ProjectState,
    pub plugin_catalog: Vec<plugins::PluginEntry>,
    pub built_in_instruments: Vec<BuiltInInstrumentSummary>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub runtime_projection: RuntimeProjectionStatus,
    pub audio_status: AudioStatus,
    pub recovery: ProjectRecoveryState,
    pub safe_mode: bool,
    pub data_root: PathBuf,
}

pub(crate) struct HostState {
    pub(super) _lease: DataRootLease,
    pub(super) identity: HostIdentity,
    pub(crate) data_root: PathBuf,
    pub(super) core: Arc<AppCore<AudioSupervisor>>,
    pub(super) project_store: ProjectStore,
    pub(super) runtime: Arc<RuntimeReconciler<AudioSupervisor>>,
    pub(super) built_in_instruments: Arc<BuiltInInstrumentCatalog>,
    pub(super) events: SharedHostEventSink,
    pub(super) event_hub: Arc<HostEventHub>,
    pub(super) binaries: RuntimeBinaries,
    pub(super) render_worker: render::RenderWorker,
    pub(super) jobs: JobRegistry,
    pub(super) audio_preferences: Mutex<AudioPreferences>,
    pub(super) recording_gate: Mutex<()>,
    pub(super) _command_gate: Mutex<()>,
    pub(super) startup_gate: Mutex<()>,
    pub(super) lifecycle_gate: RwLock<()>,
    pub(super) shutting_down: AtomicBool,
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
        self.core
            .canonical_state()
            .map_err(|error| HostError::State(error.to_string()))
    }

    pub(super) fn bootstrap(&self) -> Result<HostBootstrap, HostError> {
        let recovered_from_generation = self.core.recovered_from_generation();
        let storage = self
            .project_store
            .active_session_store()
            .map_err(|error| HostError::State(error.to_string()))?;
        Ok(HostBootstrap {
            canonical: self.canonical()?,
            project_state: projects::state(&self.project_store).map_err(HostError::State)?,
            plugin_catalog: plugins::load(&self.data_root)
                .map_err(|error| HostError::State(error.to_string()))?,
            built_in_instruments: self.built_in_instruments.summaries(),
            runtime_started: self.core.audio().startup_completed(),
            runtime_startup_finished: self.core.audio().startup_finished(),
            runtime_projection: self.runtime.status(),
            audio_status: self
                .core
                .audio()
                .status()
                .map_err(|error| HostError::State(error.to_string()))?,
            recovery: projects::recovery(&storage, recovered_from_generation)
                .map_err(HostError::State)?,
            safe_mode: self.core.safe_mode(),
            data_root: self.data_root.clone(),
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
