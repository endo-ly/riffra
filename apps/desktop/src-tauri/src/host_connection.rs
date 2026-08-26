//! Desktop-side ownership and routing for the currently connected Host.

use crate::model::{BootstrapState, RecoveryCandidate};
use riffra_control::{
    ControlCommand, ControlRequest, ControlResponse, HostEventFrame, LocalHostClient,
    LocalHostDiscovery, LocalHostEventStreamHandle, LocalHostRegistry, new_instance_id,
};
use riffra_runtime::{
    DawHost, HostBootstrap, HostConfig, HostEvent, HostEventSink, RuntimeBinaries,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
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
        stop: Arc<AtomicBool>,
        close: LocalHostEventStreamHandle,
    },
    Disconnected {
        data_root: Option<PathBuf>,
        instance_id: Option<String>,
        pid: Option<u32>,
        generation: u64,
        reason: String,
    },
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

/// Converts Host events from every source into generation-aware Tauri events.
struct DesktopEventRouter {
    app: AppHandle,
    active_generation: AtomicU64,
    discarded_through: AtomicU64,
    delivery: Mutex<()>,
    pending: Mutex<BTreeMap<u64, Vec<HostEventFrame>>>,
}

impl DesktopEventRouter {
    fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
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
        let _ = self.app.emit("host-connection-changed", connection);
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
        let _ = self.app.emit(
            "host-connection-changed",
            json!({
                "state": connection,
                "bootstrap": bootstrap,
            }),
        );
    }

    fn emit_host_event(&self, generation: u64, event: HostEvent) {
        let frame = host_event_frame(event);
        self.receive(generation, frame);
    }

    fn emit_frame(&self, frame: HostEventFrame) {
        if let Err(error) = self.app.emit(&frame.event, frame.payload) {
            tracing::debug!(error = %error, "Desktop Host event could not be delivered");
        }
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
    operation_barrier: RwLock<()>,
    switch_mutex: Mutex<()>,
    next_generation: AtomicU64,
    settings: EmbeddedHostSettings,
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
        let router = DesktopEventRouter::new(app);
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
            operation_barrier: RwLock::new(()),
            switch_mutex: Mutex::new(()),
            next_generation: AtomicU64::new(1),
            settings,
            router,
            messages,
        });
        manager.start_message_loop(receiver);
        manager.initialize()
    }

    fn initialize(self: &Arc<Self>) -> Result<Arc<Self>, String> {
        let generation = 1;
        let embedded = self.prepare_embedded(generation);
        let prepared = match embedded {
            Ok(prepared) => prepared,
            Err(error) if is_data_root_conflict(&error) => {
                self.prepare_attached_data_root(&self.settings.data_root, generation)
                    .map_err(|attach_error| {
                        format!(
                            "Desktop Data Root is owned by another Host, but that Host could not be attached: {attach_error}"
                        )
                    })?
            }
            Err(error) => return Err(error),
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
                stop,
                close,
                ..
            } => {
                stop.store(true, Ordering::Release);
                close.close();
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
        let connection = HostConnectionBootstrap {
            state: state.clone(),
            bootstrap: self.to_desktop_bootstrap(&state, &prepared.bootstrap),
        };
        self.router.activate(state.generation, &connection);
        drop(reader_status_guard);
        Ok(())
    }

    fn prepare_embedded(&self, generation: u64) -> Result<PreparedHost, String> {
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
        )
        .map_err(|error| error.to_string())?;
        let bootstrap = match host.bootstrap() {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                host.shutdown();
                return Err(error.to_string());
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
        let close = event_stream
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
                    match event_stream.next() {
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
            })
            .map_err(|error| format!("Host event reader could not start: {error}"));
        if let Err(error) = reader {
            stop.store(true, Ordering::Release);
            close.close();
            return Err(error);
        }
        let bootstrap = match self.request_bootstrap(&client) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                stop.store(true, Ordering::Release);
                close.close();
                return Err(error.to_string());
            }
        };
        if let Some(error) = reader_error
            .lock()
            .expect("Host event reader status was poisoned")
            .clone()
        {
            stop.store(true, Ordering::Release);
            close.close();
            return Err(format!("Host event connection failed: {error}"));
        }
        Ok(PreparedHost {
            active: ActiveHost::Attached {
                client,
                data_root,
                instance_id: descriptor.instance_id,
                pid: descriptor.pid,
                generation,
                stop,
                close,
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
            HostTarget::Embedded => self.prepare_embedded(generation),
            HostTarget::DataRoot { data_root } => {
                self.prepare_attached_data_root(Path::new(&data_root), generation)
            }
            HostTarget::Registration { instance_id } => {
                let registry = LocalHostRegistry::current_user();
                let discovery = registry
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
        let discovered = LocalHostRegistry::current_user()
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

    /// Switches Hosts transactionally. The current Host remains active if prepare fails.
    pub fn switch(self: &Arc<Self>, target: HostTarget) -> Result<HostConnectionBootstrap, String> {
        let _switch = self
            .switch_mutex
            .lock()
            .map_err(|_| "Host switch lock was poisoned".to_string())?;
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
            plugin_catalog: bootstrap.plugin_catalog.clone(),
            runtime_started: bootstrap.runtime_started,
            runtime_startup_finished: bootstrap.runtime_startup_finished,
            recovered_from_generation: bootstrap.recovered_from_generation,
            safe_mode: bootstrap.safe_mode,
            native_available: true,
            recovery_candidates: map_recovery_candidates(bootstrap.recovery_candidates.clone()),
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
        ActiveHost::Attached { stop, close, .. } => {
            stop.store(true, Ordering::Release);
            close.close();
        }
        ActiveHost::Disconnected { .. } => {}
    }
}

fn host_info(discovery: LocalHostDiscovery) -> Result<LocalHostInfo, String> {
    let registration = discovery.registration;
    let response = discovery
        .client
        .request(&ControlRequest::new(
            format!("desktop-info-{}", new_instance_id()),
            ControlCommand::new("host.bootstrap", json!({})),
            None,
        ))
        .map_err(|error| error.to_string())?;
    let bootstrap: HostBootstrap = response_value(response)?;
    let status = match bootstrap.audio_status.state {
        riffra_runtime::AudioState::Faulted => "Faulted",
        riffra_runtime::AudioState::Muted => "Muted",
        riffra_runtime::AudioState::Starting => "Starting",
        riffra_runtime::AudioState::Offline => "Offline",
        riffra_runtime::AudioState::Ready => "Ready",
    };
    Ok(LocalHostInfo {
        instance_id: registration.instance_id,
        pid: registration.pid,
        data_root: registration.data_root.to_string_lossy().into_owned(),
        started_at_ms: registration.started_at_ms,
        project_name: bootstrap.canonical.session.settings.project_name,
        safe_mode: bootstrap.safe_mode,
        status: status.into(),
    })
}

fn is_data_root_conflict(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.starts_with("data root could not be opened:") && error.contains("already in use")
}

fn is_telemetry_event(event: &str) -> bool {
    matches!(event, "audio-meters" | "transport-status")
}

fn host_event_frame(event: HostEvent) -> HostEventFrame {
    match event {
        HostEvent::CanonicalStateChanged(value) => {
            HostEventFrame::new("canonical-state-changed", json!(value))
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
