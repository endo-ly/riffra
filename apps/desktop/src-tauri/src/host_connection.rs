//! Desktop-side ownership and routing for the currently connected Host.

use crate::model::{BootstrapState, ProjectRecoveryState, RecoveryCandidate};
use riffra_control::{
    ControlCommand, ControlRequest, ControlResponse, HostEventFrame, LocalHostClient,
    LocalHostDiscovery, LocalHostEventStreamHandle, LocalHostRegistry, new_instance_id,
};
use riffra_runtime::{
    AudioStatus, DawHost, HostBootstrap, HostConfig, HostError, HostEvent, HostEventSink,
    RuntimeBinaries, command_requires_project_id,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

const EVENT_BUFFER_LIMIT: usize = 512;

/// The kind of Host currently used by the Desktop shell.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, TS)]
#[serde(rename_all = "lowercase")]
pub enum HostConnectionMode {
    Embedded,
    Attached,
    Disconnected,
}

/// A serializable snapshot of the current Host connection.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostConnectionState {
    pub mode: HostConnectionMode,
    pub generation: u64,
    pub data_root: Option<String>,
    pub instance_id: Option<String>,
    pub pid: Option<u32>,
    pub reason: Option<String>,
}

/// A Host target selected by the Desktop Host Selector.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostTarget {
    Embedded,
    Registration { instance_id: String },
    DataRoot { data_root: String },
}

/// One verified Host available to the Desktop Host Selector.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHostInfo {
    pub instance_id: String,
    pub pid: u32,
    pub data_root: String,
    pub started_at_ms: u64,
    pub project_name: Option<String>,
    pub safe_mode: bool,
    pub status: String,
}

/// The new Host state and bootstrap returned after a successful switch.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostConnectionBootstrap {
    pub state: HostConnectionState,
    pub bootstrap: BootstrapState,
}

enum ActiveHost {
    Embedded {
        host: DawHost,
        generation: u64,
    },
    Attached {
        client: Arc<LocalHostClient>,
        data_root: PathBuf,
        instance_id: String,
        pid: u32,
        generation: u64,
        events: AttachedEventStream,
    },
    Disconnected {
        data_root: Option<PathBuf>,
        instance_id: Option<String>,
        pid: Option<u32>,
        generation: u64,
        reason: String,
    },
}

/// The Desktop-side half of one attached Host event connection.
struct AttachedEventStream {
    stop: Arc<AtomicBool>,
    closer: LocalHostEventStreamHandle,
    reader: Mutex<Option<thread::JoinHandle<()>>>,
}

impl AttachedEventStream {
    /// Closes the connection and stops a blocked reader promptly.
    ///
    /// Windows pipe reads here are synchronous, so closing the stream alone
    /// cannot wake the reader; its pending read is cancelled on its own
    /// thread. The bounded retries close the window where a reader re-enters
    /// its blocking read after one cancellation.
    fn halt(&self) {
        self.stop.store(true, Ordering::Release);
        let Ok(reader) = self.reader.lock() else {
            return;
        };
        let Some(handle) = reader.as_ref() else {
            return;
        };
        for _ in 0..100 {
            self.closer.close();
            #[cfg(windows)]
            riffra_control::transport::cancel_synchronous_io(handle);
            if handle.is_finished() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl ActiveHost {
    fn state(&self) -> HostConnectionState {
        match self {
            Self::Embedded { host, generation } => HostConnectionState {
                mode: HostConnectionMode::Embedded,
                generation: *generation,
                data_root: Some(host.data_root().to_string_lossy().into_owned()),
                instance_id: Some(host.identity().instance_id.clone()),
                pid: Some(host.identity().pid),
                reason: None,
            },
            Self::Attached {
                data_root,
                instance_id,
                pid,
                generation,
                ..
            } => HostConnectionState {
                mode: HostConnectionMode::Attached,
                generation: *generation,
                data_root: Some(data_root.to_string_lossy().into_owned()),
                instance_id: Some(instance_id.clone()),
                pid: Some(*pid),
                reason: None,
            },
            Self::Disconnected {
                data_root,
                instance_id,
                pid,
                generation,
                reason,
            } => HostConnectionState {
                mode: HostConnectionMode::Disconnected,
                generation: *generation,
                data_root: data_root
                    .as_ref()
                    .map(|root| root.to_string_lossy().into_owned()),
                instance_id: instance_id.clone(),
                pid: *pid,
                reason: Some(reason.clone()),
            },
        }
    }
}

struct PreparedHost {
    active: ActiveHost,
    bootstrap: HostBootstrap,
    reader_error: Option<Arc<Mutex<Option<String>>>>,
}

#[derive(Debug)]
enum BootstrapRequestError {
    Connection(String),
    Response(String),
}

impl std::fmt::Display for BootstrapRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection(error) => write!(formatter, "Host connection failed: {error}"),
            Self::Response(error) => formatter.write_str(error),
        }
    }
}

enum ConnectionMessage {
    Event {
        generation: u64,
        frame: HostEventFrame,
    },
    Disconnected {
        generation: u64,
        reason: String,
    },
}

/// Boundary that delivers generation-aware connection events to the shell,
/// kept abstract so the switching logic is testable without a Tauri app.
trait ConnectionEventOutlet: Send + Sync + 'static {
    fn emit_host_event(&self, event: &str, payload: Value);
    fn emit_connection_changed(&self, connection: &HostConnectionBootstrap);
    fn emit_invalidated(&self, state: &HostConnectionState, bootstrap: Option<&BootstrapState>);
}

/// Delivers Host events to the WebView through Tauri.
struct TauriConnectionOutlet {
    app: AppHandle,
}

impl ConnectionEventOutlet for TauriConnectionOutlet {
    fn emit_host_event(&self, event: &str, payload: Value) {
        if let Err(error) = self.app.emit(event, payload) {
            tracing::debug!(error = %error, "Desktop Host event could not be delivered");
        }
    }

    fn emit_connection_changed(&self, connection: &HostConnectionBootstrap) {
        let _ = self.app.emit("host-connection-changed", connection);
    }

    fn emit_invalidated(&self, state: &HostConnectionState, bootstrap: Option<&BootstrapState>) {
        let _ = self.app.emit(
            "host-connection-changed",
            json!({"state": state, "bootstrap": bootstrap}),
        );
    }
}

/// Converts Host events from every source into generation-aware shell events.
struct DesktopEventRouter {
    outlet: Arc<dyn ConnectionEventOutlet>,
    active_generation: AtomicU64,
    discarded_through: AtomicU64,
    delivery: Mutex<()>,
    pending: Mutex<BTreeMap<u64, Vec<HostEventFrame>>>,
}

impl DesktopEventRouter {
    fn new(outlet: Arc<dyn ConnectionEventOutlet>) -> Arc<Self> {
        Arc::new(Self {
            outlet,
            active_generation: AtomicU64::new(0),
            discarded_through: AtomicU64::new(0),
            delivery: Mutex::new(()),
            pending: Mutex::new(BTreeMap::new()),
        })
    }

    fn receive(&self, generation: u64, frame: HostEventFrame) {
        if matches!(
            frame.event.as_str(),
            "track-plugin-state-changed" | "track-plugin-parameter-changed"
        ) {
            return;
        }
        let _delivery = self
            .delivery
            .lock()
            .expect("Host event delivery lock was poisoned");
        if generation <= self.discarded_through.load(Ordering::Acquire) {
            return;
        }
        match generation.cmp(&self.active_generation.load(Ordering::Acquire)) {
            std::cmp::Ordering::Equal => self.emit_frame(frame),
            std::cmp::Ordering::Greater => {
                let mut pending = self.pending.lock().expect("Host event buffer was poisoned");
                let queue = pending.entry(generation).or_default();
                if queue.len() < EVENT_BUFFER_LIMIT || !is_telemetry_event(&frame.event) {
                    queue.push(frame);
                } else {
                    if let Some(previous) = queue
                        .iter_mut()
                        .rev()
                        .find(|previous| previous.event == frame.event)
                    {
                        *previous = frame;
                    }
                }
            }
            std::cmp::Ordering::Less => {}
        }
    }

    fn discard(&self, generation: u64) {
        let _delivery = self
            .delivery
            .lock()
            .expect("Host event delivery lock was poisoned");
        self.discarded_through
            .fetch_max(generation, Ordering::AcqRel);
        self.pending
            .lock()
            .expect("Host event buffer was poisoned")
            .remove(&generation);
    }

    fn activate(&self, generation: u64, connection: &HostConnectionBootstrap) {
        let _delivery = self
            .delivery
            .lock()
            .expect("Host event delivery lock was poisoned");
        self.active_generation.store(generation, Ordering::Release);
        self.pending
            .lock()
            .expect("Host event buffer was poisoned")
            .retain(|queued_generation, _| *queued_generation >= generation);
        self.outlet.emit_connection_changed(connection);
        let queued = self
            .pending
            .lock()
            .expect("Host event buffer was poisoned")
            .remove(&generation)
            .unwrap_or_default();
        for frame in queued {
            self.emit_frame(frame);
        }
    }

    fn invalidate(
        &self,
        generation: u64,
        connection: &HostConnectionState,
        bootstrap: Option<&BootstrapState>,
    ) {
        let _delivery = self
            .delivery
            .lock()
            .expect("Host event delivery lock was poisoned");
        self.active_generation.store(generation, Ordering::Release);
        self.pending
            .lock()
            .expect("Host event buffer was poisoned")
            .retain(|queued_generation, _| *queued_generation >= generation);
        self.outlet.emit_invalidated(connection, bootstrap);
    }

    fn emit_host_event(&self, generation: u64, event: HostEvent) {
        let frame = host_event_frame(event);
        self.receive(generation, frame);
    }

    fn emit_frame(&self, frame: HostEventFrame) {
        self.outlet
            .emit_host_event(&frame.event.clone(), frame.payload);
    }
}

struct EmbeddedEventSink {
    router: Arc<DesktopEventRouter>,
    generation: u64,
}

impl HostEventSink for EmbeddedEventSink {
    fn emit(&self, event: HostEvent) {
        self.router.emit_host_event(self.generation, event);
    }
}

/// Coordinates Embedded, Attached, and Disconnected Host lifecycles.
pub struct HostConnectionManager {
    active: RwLock<ActiveHost>,
    bootstrap: RwLock<Option<HostBootstrap>>,
    active_project_id: RwLock<Option<String>>,
    operation_barrier: RwLock<()>,
    switch_mutex: Mutex<()>,
    next_generation: AtomicU64,
    settings: EmbeddedHostSettings,
    registry: LocalHostRegistry,
    router: Arc<DesktopEventRouter>,
    messages: Sender<ConnectionMessage>,
}

#[derive(Clone)]
pub struct EmbeddedHostSettings {
    pub data_root: PathBuf,
    pub safe_mode: bool,
    pub binaries: RuntimeBinaries,
}

impl HostConnectionManager {
    /// Creates the manager and starts the Desktop's initial Host.
    pub fn open(app: AppHandle, settings: EmbeddedHostSettings) -> Result<Arc<Self>, String> {
        Self::start(
            Arc::new(TauriConnectionOutlet { app }),
            settings,
            LocalHostRegistry::current_user(),
        )
    }

    fn start(
        outlet: Arc<dyn ConnectionEventOutlet>,
        settings: EmbeddedHostSettings,
        registry: LocalHostRegistry,
    ) -> Result<Arc<Self>, String> {
        let router = DesktopEventRouter::new(outlet);
        let (messages, receiver) = mpsc::channel();
        let manager = Arc::new(Self {
            active: RwLock::new(ActiveHost::Disconnected {
                data_root: Some(settings.data_root.clone()),
                instance_id: None,
                pid: None,
                generation: 0,
                reason: "Host is starting".into(),
            }),
            bootstrap: RwLock::new(None),
            active_project_id: RwLock::new(None),
            operation_barrier: RwLock::new(()),
            switch_mutex: Mutex::new(()),
            next_generation: AtomicU64::new(1),
            settings,
            registry,
            router,
            messages,
        });
        manager.start_message_loop(receiver);
        manager.initialize()
    }

    fn initialize(self: &Arc<Self>) -> Result<Arc<Self>, String> {
        let generation = 1;
        let prepared = match self.prepare_embedded(generation) {
            Ok(prepared) => prepared,
            Err(HostError::DataRootInUse) => {
                // The Desktop Data Root is owned by a live external Host;
                // attach to it instead of opening the root again.
                self.prepare_attached_data_root(&self.settings.data_root, generation)
                    .map_err(|attach_error| {
                        format!(
                            "Desktop Data Root is owned by another Host, but that Host could not be attached: {attach_error}"
                        )
                    })?
            }
            Err(error) => return Err(error.to_string()),
        };
        self.install_prepared(prepared)?;
        Ok(Arc::clone(self))
    }

    fn start_message_loop(self: &Arc<Self>, receiver: Receiver<ConnectionMessage>) {
        let weak = Arc::downgrade(self);
        let _ = thread::Builder::new()
            .name("riffra-desktop-host-events".into())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    let Some(manager) = weak.upgrade() else {
                        break;
                    };
                    manager.handle_message(message);
                }
            });
    }

    fn handle_message(&self, message: ConnectionMessage) {
        match message {
            ConnectionMessage::Event { generation, frame } => {
                if frame.event == "project-activated"
                    && self.active_state().generation == generation
                    && let Some(project_id) =
                        frame.payload["projectState"]["activeProjectId"].as_str()
                {
                    *self
                        .active_project_id
                        .write()
                        .expect("active Project lock was poisoned") = Some(project_id.to_owned());
                }
                self.router.receive(generation, frame);
            }
            ConnectionMessage::Disconnected { generation, reason } => {
                self.handle_disconnect(generation, reason);
            }
        }
    }

    fn handle_disconnect(&self, generation: u64, reason: String) {
        let Ok(_switch) = self.switch_mutex.lock() else {
            return;
        };
        let Ok(_write) = self.operation_barrier.write() else {
            return;
        };
        let mut active = self
            .active
            .write()
            .expect("Host connection lock was poisoned");
        let should_disconnect = matches!(
            &*active,
            ActiveHost::Attached {
                generation: active_generation,
                ..
            } if *active_generation == generation
        );
        if !should_disconnect {
            return;
        }
        let disconnected_generation = self.allocate_generation();
        let previous = std::mem::replace(
            &mut *active,
            ActiveHost::Disconnected {
                data_root: None,
                instance_id: None,
                pid: None,
                generation: disconnected_generation,
                reason: reason.clone(),
            },
        );
        let (data_root, instance_id, pid) = match previous {
            ActiveHost::Attached {
                data_root,
                instance_id,
                pid,
                events,
                ..
            } => {
                events.halt();
                (Some(data_root), Some(instance_id), Some(pid))
            }
            other => {
                *active = other;
                return;
            }
        };
        if let ActiveHost::Disconnected {
            data_root: disconnected_root,
            instance_id: disconnected_instance,
            pid: disconnected_pid,
            ..
        } = &mut *active
        {
            *disconnected_root = data_root;
            *disconnected_instance = instance_id;
            *disconnected_pid = pid;
        }
        let state = active.state();
        drop(active);
        drop(_write);
        let bootstrap = self.desktop_bootstrap().ok();
        self.router
            .invalidate(state.generation, &state, bootstrap.as_ref());
    }

    fn install_prepared(&self, prepared: PreparedHost) -> Result<(), String> {
        let reader_status = prepared.reader_error.clone();
        let reader_status_guard = reader_status.as_ref().map(|status| {
            status
                .lock()
                .expect("Host event reader status was poisoned")
        });
        if let Some(error) = reader_status_guard
            .as_ref()
            .and_then(|status| (**status).clone())
        {
            drop(reader_status_guard);
            shutdown_old(prepared.active);
            return Err(format!("Host event connection failed: {error}"));
        }
        let state = prepared.active.state();
        *self
            .active
            .write()
            .expect("Host connection lock was poisoned") = prepared.active;
        *self
            .bootstrap
            .write()
            .expect("Host bootstrap lock was poisoned") = Some(prepared.bootstrap.clone());
        *self
            .active_project_id
            .write()
            .expect("active Project lock was poisoned") =
            Some(prepared.bootstrap.project_state.active_project_id.clone());
        let connection = HostConnectionBootstrap {
            state: state.clone(),
            bootstrap: self.to_desktop_bootstrap(&state, &prepared.bootstrap),
        };
        self.router.activate(state.generation, &connection);
        drop(reader_status_guard);
        Ok(())
    }

    fn prepare_embedded(&self, generation: u64) -> Result<PreparedHost, HostError> {
        let host = DawHost::open(
            HostConfig {
                data_root: self.settings.data_root.clone(),
                safe_mode: self.settings.safe_mode,
                binaries: self.settings.binaries.clone(),
            },
            Arc::new(EmbeddedEventSink {
                router: Arc::clone(&self.router),
                generation,
            }),
        )?;
        let bootstrap = match host.bootstrap() {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                host.shutdown();
                return Err(error);
            }
        };
        Ok(PreparedHost {
            active: ActiveHost::Embedded { host, generation },
            bootstrap,
            reader_error: None,
        })
    }

    fn prepare_attached_data_root(
        self: &Arc<Self>,
        data_root: &Path,
        generation: u64,
    ) -> Result<PreparedHost, String> {
        let client =
            LocalHostClient::connect_data_root(data_root).map_err(|error| error.to_string())?;
        self.prepare_attached(client, data_root.to_path_buf(), generation)
    }

    fn prepare_attached(
        self: &Arc<Self>,
        client: LocalHostClient,
        data_root: PathBuf,
        generation: u64,
    ) -> Result<PreparedHost, String> {
        let client = Arc::new(client);
        let descriptor = client.descriptor().clone();
        let mut event_stream = client
            .open_event_stream()
            .map_err(|error| error.to_string())?;
        let closer = event_stream
            .close_handle()
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let reader_error = Arc::new(Mutex::new(None));
        let reader_failure = Arc::clone(&reader_error);
        let messages = self.messages.clone();
        let reader = thread::Builder::new()
            .name(format!("riffra-host-events-{generation}"))
            .spawn(move || {
                loop {
                    match event_stream.recv() {
                        Ok(frame) if !reader_stop.load(Ordering::Acquire) => {
                            if messages
                                .send(ConnectionMessage::Event { generation, frame })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(_) => break,
                        Err(error) => {
                            if !reader_stop.load(Ordering::Acquire) {
                                let reason = error.to_string();
                                if let Ok(mut failure) = reader_failure.lock() {
                                    *failure = Some(reason.clone());
                                }
                                let _ = messages
                                    .send(ConnectionMessage::Disconnected { generation, reason });
                            }
                            break;
                        }
                    }
                }
            });
        let reader = match reader {
            Ok(reader) => reader,
            // A failed spawn drops the reader closure and its stream, which
            // closes the event connection.
            Err(error) => return Err(format!("Host event reader could not start: {error}")),
        };
        let events = AttachedEventStream {
            stop,
            closer,
            reader: Mutex::new(Some(reader)),
        };
        let bootstrap = match self.request_bootstrap(&client) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                events.halt();
                return Err(error.to_string());
            }
        };
        if let Some(error) = reader_error
            .lock()
            .expect("Host event reader status was poisoned")
            .clone()
        {
            events.halt();
            return Err(format!("Host event connection failed: {error}"));
        }
        Ok(PreparedHost {
            active: ActiveHost::Attached {
                client,
                data_root,
                instance_id: descriptor.instance_id,
                pid: descriptor.pid,
                generation,
                events,
            },
            bootstrap,
            reader_error: Some(reader_error),
        })
    }

    fn request_bootstrap(
        &self,
        client: &LocalHostClient,
    ) -> Result<HostBootstrap, BootstrapRequestError> {
        let response = client
            .request(&ControlRequest::new(
                format!("desktop-bootstrap-{}", new_instance_id()),
                ControlCommand::new("host.bootstrap", json!({})),
                None,
            ))
            .map_err(|error| BootstrapRequestError::Connection(error.to_string()))?;
        response_value(response).map_err(BootstrapRequestError::Response)
    }

    fn prepare_target(
        self: &Arc<Self>,
        target: HostTarget,
        generation: u64,
    ) -> Result<PreparedHost, String> {
        match target {
            HostTarget::Embedded => self
                .prepare_embedded(generation)
                .map_err(|error| error.to_string()),
            HostTarget::DataRoot { data_root } => {
                self.prepare_attached_data_root(Path::new(&data_root), generation)
            }
            HostTarget::Registration { instance_id } => {
                let discovery = self
                    .registry
                    .discover()
                    .map_err(|error| format!("Local Host discovery failed: {error}"))?
                    .into_iter()
                    .find(|host| host.registration.instance_id == instance_id)
                    .ok_or_else(|| format!("Local Host is no longer available: {instance_id}"))?;
                self.prepare_discovery(discovery, generation)
            }
        }
    }

    fn prepare_discovery(
        self: &Arc<Self>,
        discovery: LocalHostDiscovery,
        generation: u64,
    ) -> Result<PreparedHost, String> {
        self.prepare_attached(
            discovery.client,
            discovery.registration.data_root,
            generation,
        )
    }

    /// Returns the current connection snapshot.
    pub fn state(&self) -> HostConnectionState {
        let _read = self
            .operation_barrier
            .read()
            .expect("Host operation barrier was poisoned");
        self.active_state()
    }

    fn active_state(&self) -> HostConnectionState {
        self.active
            .read()
            .expect("Host connection lock was poisoned")
            .state()
    }

    /// Lists currently live Hosts after verifying their command handshake.
    pub fn list_local_hosts(&self) -> Result<Vec<LocalHostInfo>, String> {
        let current = self.state();
        let embedded_instance = (current.mode == HostConnectionMode::Embedded)
            .then_some(current.instance_id)
            .flatten();
        let discovered = self
            .registry
            .discover()
            .map_err(|error| format!("Local Host discovery failed: {error}"))?;
        discovered
            .into_iter()
            .filter(|host| {
                Some(host.registration.instance_id.as_str()) != embedded_instance.as_deref()
            })
            .map(host_info)
            .collect()
    }

    /// Reports whether the currently connected Host is capturing input.
    fn current_recording_active(&self) -> Result<bool, String> {
        let active = self
            .active
            .read()
            .map_err(|_| "Host connection lock was poisoned".to_string())?;
        Ok(match &*active {
            ActiveHost::Embedded { host, .. } => host.recording_active(),
            ActiveHost::Attached { client, .. } => attached_recording_active(client),
            ActiveHost::Disconnected { .. } => false,
        })
    }

    /// Switches Hosts transactionally. The current Host remains active if prepare fails.
    ///
    /// Switching away would shut the current Host down mid-capture, so an
    /// active recording rejects the switch until the user stops it.
    pub fn switch(self: &Arc<Self>, target: HostTarget) -> Result<HostConnectionBootstrap, String> {
        let _switch = self
            .switch_mutex
            .lock()
            .map_err(|_| "Host switch lock was poisoned".to_string())?;
        ensure_no_active_recording(self.current_recording_active()?)?;
        let generation = self.allocate_generation();
        let prepared = match self.prepare_target(target, generation) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.router.discard(generation);
                return Err(error);
            }
        };
        let _write = self
            .operation_barrier
            .write()
            .map_err(|_| "Host operation barrier was poisoned".to_string())?;
        // Re-check after target preparation so a recording that started while
        // the target was bootstrapping cannot be cut off by the swap.
        if let Err(error) = ensure_no_active_recording(self.current_recording_active()?) {
            self.router.discard(generation);
            drop(_write);
            shutdown_old(prepared.active);
            return Err(error);
        }
        let reader_status = prepared.reader_error.clone();
        let reader_status_guard = reader_status.as_ref().map(|status| {
            status
                .lock()
                .expect("Host event reader status was poisoned")
        });
        if let Some(error) = reader_status_guard
            .as_ref()
            .and_then(|status| (**status).clone())
        {
            drop(reader_status_guard);
            self.router.discard(generation);
            drop(_write);
            shutdown_old(prepared.active);
            return Err(format!("Host event connection failed: {error}"));
        }
        let old = {
            let mut active = self
                .active
                .write()
                .map_err(|_| "Host connection lock was poisoned".to_string())?;
            std::mem::replace(
                &mut *active,
                ActiveHost::Disconnected {
                    data_root: None,
                    instance_id: None,
                    pid: None,
                    generation,
                    reason: "Host switch in progress".into(),
                },
            )
        };
        let state = prepared.active.state();
        *self
            .active
            .write()
            .map_err(|_| "Host connection lock was poisoned".to_string())? = prepared.active;
        *self
            .bootstrap
            .write()
            .map_err(|_| "Host bootstrap lock was poisoned".to_string())? =
            Some(prepared.bootstrap.clone());
        *self
            .active_project_id
            .write()
            .map_err(|_| "active Project lock was poisoned".to_string())? =
            Some(prepared.bootstrap.project_state.active_project_id.clone());
        let result = HostConnectionBootstrap {
            state: state.clone(),
            bootstrap: self.to_desktop_bootstrap(&state, &prepared.bootstrap),
        };
        self.router.activate(state.generation, &result);
        drop(reader_status_guard);
        drop(_write);
        shutdown_old(old);
        Ok(result)
    }

    /// Reconnects through the last Data Root rather than trusting an old endpoint.
    pub fn reconnect(self: &Arc<Self>) -> Result<HostConnectionBootstrap, String> {
        let state = self.state();
        let data_root = state
            .data_root
            .ok_or_else(|| "There is no Host Data Root to reconnect".to_string())?;
        self.switch(HostTarget::DataRoot { data_root })
    }

    /// Runs one Host-owned operation through the current Host.
    pub fn dispatch<T: DeserializeOwned>(&self, command: &str, params: Value) -> Result<T, String> {
        let _read = self
            .operation_barrier
            .read()
            .map_err(|_| "Host operation barrier was poisoned".to_string())?;
        let request = ControlRequest::new(
            format!("desktop-command-{}", new_instance_id()),
            ControlCommand::new(command, params),
            None,
        );
        let request = if command_requires_project_id(command) {
            let project_id = self
                .active_project_id
                .read()
                .map_err(|_| "active Project lock was poisoned".to_string())?
                .clone()
                .ok_or_else(|| "active Project is not available".to_string())?;
            request.with_expected_project_id(project_id)
        } else {
            request
        };
        let (response, attached_generation) = {
            let active = self
                .active
                .read()
                .map_err(|_| "Host connection lock was poisoned".to_string())?;
            match &*active {
                ActiveHost::Embedded { host, .. } => (Ok(host.dispatch_control(request)), None),
                ActiveHost::Attached {
                    client, generation, ..
                } => (
                    client.request(&request).map_err(|error| error.to_string()),
                    Some(*generation),
                ),
                ActiveHost::Disconnected { reason, .. } => {
                    (Err(format!("Host is disconnected: {reason}")), None)
                }
            }
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                if let Some(generation) = attached_generation {
                    let _ = self.messages.send(ConnectionMessage::Disconnected {
                        generation,
                        reason: error.clone(),
                    });
                }
                return Err(error);
            }
        };
        if response.ok
            && let Some(project_id) = response
                .result
                .as_ref()
                .and_then(|result| result.value["projectState"]["activeProjectId"].as_str())
        {
            *self
                .active_project_id
                .write()
                .map_err(|_| "active Project lock was poisoned".to_string())? =
                Some(project_id.to_owned());
        }
        response_value(response)
    }

    /// Reports whether the embedded Host requested Desktop process shutdown.
    pub fn shutdown_requested(&self) -> bool {
        self.active
            .read()
            .ok()
            .is_some_and(|active| match &*active {
                ActiveHost::Embedded { host, .. } => host.shutdown_requested(),
                _ => false,
            })
    }

    /// Shuts down only the Host owned by this Desktop process.
    pub fn shutdown(&self) {
        let Ok(_switch) = self.switch_mutex.lock() else {
            return;
        };
        let Ok(_write) = self.operation_barrier.write() else {
            return;
        };
        let Ok(mut active) = self.active.write() else {
            return;
        };
        let generation = active.state().generation;
        let previous = std::mem::replace(
            &mut *active,
            ActiveHost::Disconnected {
                data_root: None,
                instance_id: None,
                pid: None,
                generation,
                reason: "Desktop is shutting down".into(),
            },
        );
        drop(active);
        drop(_write);
        shutdown_old(previous);
    }

    pub(crate) fn desktop_bootstrap(&self) -> Result<BootstrapState, String> {
        let _read = self
            .operation_barrier
            .read()
            .map_err(|_| "Host operation barrier was poisoned".to_string())?;
        let state = self.active_state();
        let bootstrap = self.current_bootstrap_locked()?;
        *self
            .bootstrap
            .write()
            .map_err(|_| "Host bootstrap lock was poisoned".to_string())? = Some(bootstrap.clone());
        Ok(self.to_desktop_bootstrap(&state, &bootstrap))
    }

    fn current_bootstrap_locked(&self) -> Result<HostBootstrap, String> {
        let active = self
            .active
            .read()
            .map_err(|_| "Host connection lock was poisoned".to_string())?;
        match &*active {
            ActiveHost::Embedded { host, .. } => {
                host.bootstrap().map_err(|error| error.to_string())
            }
            ActiveHost::Attached {
                client, generation, ..
            } => match self.request_bootstrap(client) {
                Ok(bootstrap) => Ok(bootstrap),
                Err(BootstrapRequestError::Connection(error)) => {
                    let _ = self.messages.send(ConnectionMessage::Disconnected {
                        generation: *generation,
                        reason: error.clone(),
                    });
                    Err(error)
                }
                Err(error) => Err(error.to_string()),
            },
            ActiveHost::Disconnected { .. } => self
                .bootstrap
                .read()
                .map_err(|_| "Host bootstrap lock was poisoned".to_string())?
                .clone()
                .ok_or_else(|| "Host is disconnected and has no bootstrap".to_string()),
        }
    }

    fn allocate_generation(&self) -> u64 {
        self.next_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
    }

    fn to_desktop_bootstrap(
        &self,
        state: &HostConnectionState,
        bootstrap: &HostBootstrap,
    ) -> BootstrapState {
        BootstrapState {
            canonical: bootstrap.canonical.clone(),
            project_state: bootstrap.project_state.clone(),
            plugin_catalog: bootstrap.plugin_catalog.clone(),
            runtime_started: bootstrap.runtime_started,
            runtime_startup_finished: bootstrap.runtime_startup_finished,
            recovery: ProjectRecoveryState {
                recovered_from_generation: bootstrap.recovery.recovered_from_generation,
                recovery_candidates: map_runtime_recovery_candidates(
                    bootstrap.recovery.recovery_candidates.clone(),
                ),
            },
            safe_mode: bootstrap.safe_mode,
            native_available: true,
            data_root: bootstrap.data_root.to_string_lossy().into_owned(),
            vst3_root: default_vst3_root(),
            host_connection: state.clone(),
        }
    }
}

fn response_value<T: DeserializeOwned>(response: ControlResponse) -> Result<T, String> {
    if !response.ok {
        let error = response
            .error
            .map(|error| format!("{}: {}", error.code, error.message))
            .unwrap_or_else(|| "Host command failed".into());
        return Err(error);
    }
    let value = response
        .result
        .map(|result| result.value)
        .unwrap_or(Value::Null);
    serde_json::from_value(value)
        .map_err(|error| format!("Host response could not be decoded: {error}"))
}

fn shutdown_old(active: ActiveHost) {
    match active {
        ActiveHost::Embedded { host, .. } => host.shutdown(),
        ActiveHost::Attached { events, .. } => events.halt(),
        ActiveHost::Disconnected { .. } => {}
    }
}

/// Rejects a Host switch while the current Host is capturing input.
fn ensure_no_active_recording(recording_active: bool) -> Result<(), String> {
    if recording_active {
        Err("Stop recording before switching Hosts.".into())
    } else {
        Ok(())
    }
}

/// Queries one attached Host for its native recording state.
fn attached_recording_active(client: &LocalHostClient) -> bool {
    client
        .request(&ControlRequest::new(
            format!("desktop-recording-status-{}", new_instance_id()),
            ControlCommand::new("record.status", json!({})),
            None,
        ))
        .ok()
        .and_then(|response| response_value::<AudioStatus>(response).ok())
        .map(|status| status.recording.active)
        .unwrap_or(false)
}

/// Lightweight Host identity payload returned by `host.info`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostInfoPayload {
    project_name: Option<String>,
    safe_mode: bool,
    runtime_state: String,
}

fn host_info(discovery: LocalHostDiscovery) -> Result<LocalHostInfo, String> {
    let registration = discovery.registration;
    let response = discovery
        .client
        .request(&ControlRequest::new(
            format!("desktop-info-{}", new_instance_id()),
            ControlCommand::new("host.info", json!({})),
            None,
        ))
        .map_err(|error| error.to_string())?;
    let info: HostInfoPayload = response_value(response)?;
    Ok(LocalHostInfo {
        instance_id: registration.instance_id,
        pid: registration.pid,
        data_root: registration.data_root.to_string_lossy().into_owned(),
        started_at_ms: registration.started_at_ms,
        project_name: info.project_name,
        safe_mode: info.safe_mode,
        status: display_runtime_state(&info.runtime_state).into(),
    })
}

/// Maps the Host runtime state onto the Host Selector's display vocabulary.
fn display_runtime_state(state: &str) -> &'static str {
    match state {
        "faulted" => "Faulted",
        "muted" => "Muted",
        "starting" => "Starting",
        "offline" => "Offline",
        _ => "Ready",
    }
}

fn is_telemetry_event(event: &str) -> bool {
    matches!(event, "audio-meters" | "transport-status")
}

fn host_event_frame(event: HostEvent) -> HostEventFrame {
    match event {
        HostEvent::CanonicalStateChanged(value) => {
            HostEventFrame::new("canonical-state-changed", json!(value))
        }
        HostEvent::ProjectStateChanged(value) => {
            HostEventFrame::new("project-state-changed", json!(value))
        }
        HostEvent::ProjectActivated(value) => {
            HostEventFrame::new("project-activated", json!(value))
        }
        HostEvent::RuntimeStartupFinished { succeeded } => {
            HostEventFrame::new("runtime-startup-finished", json!({"succeeded": succeeded}))
        }
        HostEvent::RuntimeProjectionStatus(value) => {
            HostEventFrame::new("runtime-projection-status", json!(value))
        }
        HostEvent::AudioStatus(value) => HostEventFrame::new("audio-status", json!(value)),
        HostEvent::AudioMeters(value) => HostEventFrame::new("audio-meters", value),
        HostEvent::TransportStatus(value) => HostEventFrame::new("transport-status", value),
        HostEvent::RuntimeRestarted { generation } => {
            HostEventFrame::new("runtime-restarted", json!({"generation": generation}))
        }
        HostEvent::TrackPluginStateChanged(value) => {
            HostEventFrame::new("track-plugin-state-changed", value)
        }
        HostEvent::TrackPluginParameterChanged(value) => {
            HostEventFrame::new("track-plugin-parameter-changed", value)
        }
    }
}

#[cfg(test)]
pub(crate) fn map_recovery_candidates(
    candidates: Vec<riffra_host::RecoveryCandidate>,
) -> Vec<RecoveryCandidate> {
    candidates
        .into_iter()
        .map(|candidate| RecoveryCandidate {
            file_name: candidate.file_name,
            updated_at_ms: candidate.updated_at_ms,
            session_id: candidate.session_id,
            project_name: candidate.project_name,
            note: candidate.note,
        })
        .collect()
}

fn map_runtime_recovery_candidates(
    candidates: Vec<riffra_runtime::RecoveryCandidate>,
) -> Vec<RecoveryCandidate> {
    candidates
        .into_iter()
        .map(|candidate| RecoveryCandidate {
            file_name: candidate.file_name,
            updated_at_ms: candidate.updated_at_ms,
            session_id: candidate.session_id,
            project_name: candidate.project_name,
            note: candidate.note,
        })
        .collect()
}

fn default_vst3_root() -> String {
    #[cfg(windows)]
    {
        r"C:\Program Files\Common Files\VST3".into()
    }
    #[cfg(not(windows))]
    {
        "/usr/lib/vst3".into()
    }
}

#[tauri::command]
pub(crate) fn get_host_connection_state(
    state: State<'_, crate::AppState>,
) -> Result<HostConnectionState, String> {
    Ok(state.host_connection.state())
}

#[tauri::command]
pub(crate) async fn list_local_hosts(app: AppHandle) -> Result<Vec<LocalHostInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::AppState>()
            .host_connection
            .list_local_hosts()
    })
    .await
    .map_err(|error| format!("Local Host discovery failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn switch_host(
    target: HostTarget,
    app: AppHandle,
) -> Result<HostConnectionBootstrap, String> {
    let manager = Arc::clone(&app.state::<crate::AppState>().host_connection);
    tauri::async_runtime::spawn_blocking(move || manager.switch(target))
        .await
        .map_err(|error| format!("Host switch failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn reconnect_host(app: AppHandle) -> Result<HostConnectionBootstrap, String> {
    let manager = Arc::clone(&app.state::<crate::AppState>().host_connection);
    tauri::async_runtime::spawn_blocking(move || manager.reconnect())
        .await
        .map_err(|error| format!("Host reconnect failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{LocalHostRegistration, now_ms, read_endpoint};
    use riffra_runtime::NoopHostEventSink;
    use std::time::Instant;

    #[derive(Clone, Debug)]
    enum RecordedEvent {
        Frame {
            payload: Value,
        },
        ConnectionChanged {
            generation: u64,
            canonical_sequence: u64,
        },
        Invalidated {
            generation: u64,
        },
    }

    #[derive(Default)]
    struct RecordedOutlet {
        events: Mutex<Vec<RecordedEvent>>,
    }

    impl RecordedOutlet {
        fn events(&self) -> Vec<RecordedEvent> {
            self.events
                .lock()
                .expect("outlet lock was poisoned")
                .clone()
        }
    }

    impl ConnectionEventOutlet for RecordedOutlet {
        fn emit_host_event(&self, _event: &str, payload: Value) {
            self.events
                .lock()
                .expect("outlet lock was poisoned")
                .push(RecordedEvent::Frame { payload });
        }

        fn emit_connection_changed(&self, connection: &HostConnectionBootstrap) {
            self.events.lock().expect("outlet lock was poisoned").push(
                RecordedEvent::ConnectionChanged {
                    generation: connection.state.generation,
                    canonical_sequence: connection.bootstrap.canonical.sequence,
                },
            );
        }

        fn emit_invalidated(
            &self,
            state: &HostConnectionState,
            _bootstrap: Option<&BootstrapState>,
        ) {
            self.events.lock().expect("outlet lock was poisoned").push(
                RecordedEvent::Invalidated {
                    generation: state.generation,
                },
            );
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let instance = new_instance_id();
        std::env::temp_dir().join(format!(
            "riffra-hcm-{tag}-{}-{instance}",
            std::process::id()
        ))
    }

    fn offline_binaries(root: &Path) -> RuntimeBinaries {
        RuntimeBinaries::new(
            root.join("riffra-audio"),
            root.join("riffra-plugin-scan"),
            root.join("riffra-render"),
        )
    }

    fn test_manager(tag: &str) -> (Arc<HostConnectionManager>, Arc<RecordedOutlet>) {
        let outlet = Arc::new(RecordedOutlet::default());
        let data_root = temp_root(&format!("{tag}-embedded"));
        let binaries = offline_binaries(&data_root);
        let manager = HostConnectionManager::start(
            Arc::clone(&outlet) as Arc<dyn ConnectionEventOutlet>,
            EmbeddedHostSettings {
                data_root,
                safe_mode: true,
                binaries,
            },
            LocalHostRegistry::at(temp_root(&format!("{tag}-registry"))),
        )
        .expect("the embedded Host should start in Safe Mode");
        (manager, outlet)
    }

    fn standalone_host(tag: &str) -> DawHost {
        let root = temp_root(tag);
        let binaries = offline_binaries(&root);
        DawHost::open(
            HostConfig {
                data_root: root,
                safe_mode: true,
                binaries,
            },
            Arc::new(NoopHostEventSink),
        )
        .expect("a Safe Mode Host should start without native binaries")
    }

    fn register_host(manager: &HostConnectionManager, host: &DawHost) {
        let data_root = host.data_root().to_path_buf();
        let descriptor = read_endpoint(&data_root).expect("the Host publishes its endpoint");
        manager
            .registry
            .register(&LocalHostRegistration::from_descriptor(
                data_root,
                &descriptor,
                now_ms(),
            ))
            .expect("the test registry should accept the Host");
    }

    fn host_status_ok(root: &Path) -> bool {
        LocalHostClient::connect_data_root(root)
            .and_then(|client| {
                client.request(&ControlRequest::new(
                    format!("status-{}", new_instance_id()),
                    ControlCommand::new("host.status", json!({})),
                    None,
                ))
            })
            .map(|response| response.ok)
            .unwrap_or(false)
    }

    fn add_track(name: &str) -> (&'static str, Value) {
        ("track.add", json!({ "name": name, "kind": "instrument" }))
    }

    fn cleanup(paths: &[PathBuf]) {
        for path in paths {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[test]
    fn switches_between_embedded_and_attached_hosts_transactionally() {
        let (manager, outlet) = test_manager("cycle");
        let host_a = standalone_host("cycle-a");
        let host_b = standalone_host("cycle-b");
        register_host(&manager, &host_a);
        register_host(&manager, &host_b);
        let embedded_root = manager.settings.data_root.clone();

        let initial = manager.state();
        assert_eq!(initial.mode, HostConnectionMode::Embedded);

        let to_a = manager
            .switch(HostTarget::Registration {
                instance_id: host_a.identity().instance_id.clone(),
            })
            .unwrap();
        assert_eq!(to_a.state.mode, HostConnectionMode::Attached);
        assert_eq!(to_a.state.generation, 2);
        assert_eq!(
            to_a.state.instance_id.as_deref(),
            Some(host_a.identity().instance_id.as_str())
        );
        assert_eq!(
            to_a.state.data_root.as_deref(),
            Some(host_a.data_root().to_string_lossy().as_ref())
        );
        // The WebView learned about the new generation together with the
        // attached Host's own canonical sequence.
        assert!(outlet.events().iter().any(|event| matches!(
            event,
            RecordedEvent::ConnectionChanged {
                generation: 2,
                canonical_sequence: 0
            }
        )));
        let (command, params) = add_track("Routed to A");
        manager.dispatch::<Value>(command, params).unwrap();

        let to_b = manager
            .switch(HostTarget::Registration {
                instance_id: host_b.identity().instance_id.clone(),
            })
            .unwrap();
        assert_eq!(to_b.state.generation, 3);
        assert_eq!(
            to_b.state.instance_id.as_deref(),
            Some(host_b.identity().instance_id.as_str())
        );

        // The Host switched away from keeps running its own process lifetime.
        assert!(host_status_ok(host_a.data_root()));

        let back = manager.switch(HostTarget::Embedded).unwrap();
        assert_eq!(back.state.mode, HostConnectionMode::Embedded);
        assert_eq!(back.state.generation, 4);
        assert_eq!(
            back.state.data_root.as_deref(),
            Some(embedded_root.to_string_lossy().as_ref())
        );

        manager.shutdown();
        host_a.shutdown();
        host_b.shutdown();
        cleanup(&[
            embedded_root,
            host_a.data_root().to_path_buf(),
            host_b.data_root().to_path_buf(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn a_failed_target_preparation_keeps_the_current_host() {
        let (manager, _outlet) = test_manager("failed");
        let before = manager.state();
        assert_eq!(before.mode, HostConnectionMode::Embedded);

        let error = manager
            .switch(HostTarget::Registration {
                instance_id: "missing-instance".into(),
            })
            .unwrap_err();

        assert!(error.contains("missing-instance"));
        let after = manager.state();
        assert_eq!(after.mode, before.mode);
        assert_eq!(after.instance_id, before.instance_id);
        assert_eq!(after.generation, before.generation);

        manager.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn stale_generation_events_never_reach_the_frontend() {
        let (manager, outlet) = test_manager("stale-events");
        let host = standalone_host("stale-events-host");
        register_host(&manager, &host);

        let switched = manager
            .switch(HostTarget::Registration {
                instance_id: host.identity().instance_id.clone(),
            })
            .unwrap();
        let active_generation = switched.state.generation;

        manager.router.receive(
            active_generation - 1,
            HostEventFrame::new("transport-status", json!({ "position": "stale" })),
        );
        manager.router.receive(
            active_generation + 1,
            HostEventFrame::new("transport-status", json!({ "position": "future" })),
        );
        manager.router.receive(
            active_generation,
            HostEventFrame::new("transport-status", json!({ "position": "current" })),
        );

        let recorded = outlet.events();
        let positions: Vec<&str> = recorded
            .iter()
            .filter_map(|event| match event {
                RecordedEvent::Frame { payload } => payload["position"].as_str(),
                _ => None,
            })
            .collect();
        assert_eq!(positions, vec!["current"]);

        manager.shutdown();
        host.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            host.data_root().to_path_buf(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn a_lower_target_sequence_is_adopted_after_a_switch() {
        let (manager, _outlet) = test_manager("sequence");
        let (command, params) = add_track("Raises the embedded sequence");
        manager.dispatch::<Value>(command, params.clone()).unwrap();
        manager.dispatch::<Value>(command, params).unwrap();
        let embedded_sequence = manager.desktop_bootstrap().unwrap().canonical.sequence;
        assert_eq!(embedded_sequence, 2);
        let host = standalone_host("sequence-host");
        register_host(&manager, &host);

        let switched = manager
            .switch(HostTarget::Registration {
                instance_id: host.identity().instance_id.clone(),
            })
            .unwrap();

        assert_eq!(switched.state.mode, HostConnectionMode::Attached);
        assert!(switched.bootstrap.canonical.sequence < embedded_sequence);
        let (command, params) = add_track("Expected sequence 0");
        let response = LocalHostClient::connect_data_root(host.data_root())
            .unwrap()
            .request(
                &ControlRequest::new(
                    "lower-sequence-mutation",
                    ControlCommand::new(command, params),
                    Some(switched.bootstrap.canonical.sequence),
                )
                .with_expected_project_id(switched.bootstrap.project_state.active_project_id),
            )
            .unwrap();
        assert!(response.ok);
        assert_eq!(
            response.sequence,
            Some(switched.bootstrap.canonical.sequence + 1)
        );

        manager.shutdown();
        host.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            host.data_root().to_path_buf(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn an_external_host_death_moves_the_desktop_to_disconnected() {
        let (manager, outlet) = test_manager("disconnect");
        let host = standalone_host("disconnect-host");
        register_host(&manager, &host);
        manager
            .switch(HostTarget::Registration {
                instance_id: host.identity().instance_id.clone(),
            })
            .unwrap();

        host.shutdown();

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = manager.state();
            if state.mode == HostConnectionMode::Disconnected {
                assert_eq!(
                    state.instance_id.as_deref(),
                    Some(host.identity().instance_id.as_str())
                );
                assert_eq!(
                    state.data_root.as_deref(),
                    Some(host.data_root().to_string_lossy().as_ref())
                );
                assert!(state.reason.is_some());
                assert!(outlet.events().iter().any(|event| matches!(
                    event,
                    RecordedEvent::Invalidated { generation } if *generation == state.generation
                )));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the Desktop did not observe the Host death in time"
            );
            thread::sleep(Duration::from_millis(50));
        }

        manager.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            host.data_root().to_path_buf(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn an_attached_desktop_shutdown_keeps_the_external_host_alive() {
        let (manager, _outlet) = test_manager("attached-exit");
        let host = standalone_host("attached-exit-host");
        register_host(&manager, &host);
        manager
            .switch(HostTarget::Registration {
                instance_id: host.identity().instance_id.clone(),
            })
            .unwrap();

        manager.shutdown();

        assert_eq!(manager.state().mode, HostConnectionMode::Disconnected);
        assert!(host_status_ok(host.data_root()));
        host.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            host.data_root().to_path_buf(),
            manager.registry.root().to_path_buf(),
        ]);
    }

    #[test]
    fn an_embedded_desktop_shutdown_stops_the_embedded_host_orderly() {
        let (manager, _outlet) = test_manager("embedded-exit");
        let root = manager.settings.data_root.clone();

        manager.shutdown();

        assert_eq!(manager.state().mode, HostConnectionMode::Disconnected);
        assert!(!riffra_control::endpoint_path(&root).exists());
        // The Data Root lease is released, so another Host can open the root.
        let reopened = DawHost::open(
            HostConfig {
                data_root: root.clone(),
                safe_mode: true,
                binaries: offline_binaries(&root),
            },
            Arc::new(NoopHostEventSink),
        )
        .expect("the embedded Data Root should be reusable after shutdown");
        reopened.shutdown();
        cleanup(&[root, manager.registry.root().to_path_buf()]);
    }

    #[test]
    fn an_active_recording_rejects_the_switch() {
        assert_eq!(
            ensure_no_active_recording(true).unwrap_err(),
            "Stop recording before switching Hosts."
        );
        assert!(ensure_no_active_recording(false).is_ok());
    }

    #[test]
    fn an_idle_embedded_host_passes_the_recording_check() {
        let (manager, _outlet) = test_manager("idle-recording");

        assert!(!manager.current_recording_active().unwrap());

        manager.shutdown();
        cleanup(&[
            manager.settings.data_root.clone(),
            manager.registry.root().to_path_buf(),
        ]);
    }
}
