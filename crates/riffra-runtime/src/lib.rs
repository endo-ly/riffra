//! Shared live Host runtime used by the Desktop shell and the headless CLI.
//!
//! This crate deliberately contains no Tauri, WebView, or command-line
//! parser dependency. A composition root supplies executable paths and an
//! event sink, then embeds the same [`DawHost`] in either shell.

pub mod analysis;
pub mod asset;
mod audio;
mod binaries;
mod control;
mod dispatcher;
mod host;
pub mod jobs;
pub mod library;
pub mod missing;
mod model;
pub mod plugin_catalog;
pub mod plugin_validation;
pub mod plugins;
mod preferences;
pub mod projects;
pub mod recording;
pub mod render;
mod runtime;
pub mod runtime_snapshot;
pub mod session;
mod startup;

use riffra_control::HostEventFrame;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

pub use audio::{
    AudioDeviceReopenOutcome, AudioSupervisor, MuteCause, NativeAudioError, NativeAudioResult,
    RuntimeRestartHandler,
};
pub use binaries::RuntimeBinaries;
pub use dispatcher::{DispatchError, DispatchResult, Dispatcher};
pub use host::{DawHost, HostBootstrap, HostConfig, HostError};
pub use model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioAccessMode, AudioChannelInfo,
    AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe, AudioDriverInfo, AudioState,
    AudioStatus, DeviceChannels, MidiDeviceInfo, ProjectState, RecordingFinalizationOutcome,
    RecordingStatus, RecordingStopResult, RuntimeProjectionState, RuntimeProjectionStatus,
    SessionAudioPair, TrackDeviceSummary, TrackRackSummary, TrackSummary,
};
pub use preferences::{
    AudioDriverConfig, AudioPreferences, AudioPreferencesStore, access_mode_for_driver,
    active_device_matches_preferences, load_or_default,
};
pub use runtime::{
    ProjectionDriver, ProjectionStatusHook, RuntimeDriver, RuntimeError, RuntimeReconciler,
    RuntimeRecovery, TIMELINE_PREPARE_TIMEOUT, TransportDriver,
};

use riffra_core::CanonicalState;

/// Typed notifications emitted by a live Host.
#[derive(Clone, Debug)]
pub enum HostEvent {
    /// The canonical state changed after a successful Core commit.
    CanonicalStateChanged(CanonicalState),
    /// The active Project or Project list changed.
    ProjectStateChanged(ProjectState),
    /// Startup completed, with the result of the runtime handshake.
    RuntimeStartupFinished { succeeded: bool },
    /// The latest canonical arrangement projection state.
    RuntimeProjectionStatus(RuntimeProjectionStatus),
    /// Current audio device and safety state.
    AudioStatus(AudioStatus),
    /// Raw native meters retained until a stable DTO is justified.
    AudioMeters(Value),
    /// Raw transport status from the native engine.
    TransportStatus(Value),
    /// A native runtime generation was replaced.
    RuntimeRestarted { generation: u64 },
    /// Raw plugin state event from the native engine.
    TrackPluginStateChanged(Value),
    /// Raw plugin parameter event from the native engine.
    TrackPluginParameterChanged(Value),
}

impl HostEvent {
    /// Converts a Runtime event to the Domain-free local control frame.
    fn to_control_frame(&self) -> Result<HostEventFrame, serde_json::Error> {
        let (event, payload) = match self {
            Self::CanonicalStateChanged(value) => {
                ("canonical-state-changed", serde_json::to_value(value))
            }
            Self::ProjectStateChanged(value) => {
                ("project-state-changed", serde_json::to_value(value))
            }
            Self::RuntimeStartupFinished { succeeded } => (
                "runtime-startup-finished",
                Ok(serde_json::json!({"succeeded": succeeded})),
            ),
            Self::RuntimeProjectionStatus(value) => {
                ("runtime-projection-status", serde_json::to_value(value))
            }
            Self::AudioStatus(value) => ("audio-status", serde_json::to_value(value)),
            Self::AudioMeters(value) => ("audio-meters", Ok(value.clone())),
            Self::TransportStatus(value) => ("transport-status", Ok(value.clone())),
            Self::RuntimeRestarted { generation } => (
                "runtime-restarted",
                Ok(serde_json::json!({"generation": generation})),
            ),
            Self::TrackPluginStateChanged(value) => {
                ("track-plugin-state-changed", Ok(value.clone()))
            }
            Self::TrackPluginParameterChanged(value) => {
                ("track-plugin-parameter-changed", Ok(value.clone()))
            }
        };
        Ok(HostEventFrame::new(event, payload?))
    }

    fn is_coalescible(&self) -> bool {
        matches!(self, Self::AudioMeters(_) | Self::TransportStatus(_))
    }
}

/// Boundary for turning Host events into a shell-specific event system.
pub trait HostEventSink: Send + Sync + 'static {
    /// Delivers one event. Implementations must not panic when a shell is
    /// already shutting down.
    fn emit(&self, event: HostEvent);
}

/// Event sink for foreground hosts that do not have an event UI.
#[derive(Debug, Default)]
pub struct NoopHostEventSink;

impl HostEventSink for NoopHostEventSink {
    fn emit(&self, _event: HostEvent) {}
}

/// Convenient shared ownership for event sinks supplied to a Host.
pub type SharedHostEventSink = std::sync::Arc<dyn HostEventSink>;

const HOST_EVENT_QUEUE_CAPACITY: usize = 256;

fn is_telemetry_frame(event: &str) -> bool {
    matches!(event, "audio-meters" | "transport-status")
}

/// Shared pending-event queue of one local event subscriber.
///
/// Critical Host events are queued in order. Telemetry frames coalesce into a
/// latest-wins slot inside the queue, so a meter flood can never evict or
/// starve critical events. If a subscriber falls behind on critical events,
/// the queue closes so the client can reconnect and bootstrap a complete
/// snapshot instead of silently continuing after a lost event.
#[derive(Clone)]
struct EventQueue {
    state: Arc<EventQueueState>,
}

struct EventQueueState {
    pending: Mutex<PendingEvents>,
    available: Condvar,
}

#[derive(Default)]
struct PendingEvents {
    frames: VecDeque<HostEventFrame>,
    /// Set when the hub closes or the subscription is dropped.
    closed: bool,
}

impl EventQueue {
    fn new() -> Self {
        Self {
            state: Arc::new(EventQueueState {
                pending: Mutex::new(PendingEvents::default()),
                available: Condvar::new(),
            }),
        }
    }

    /// Enqueues one frame and reports whether the queue should be kept.
    fn push(&self, frame: HostEventFrame, telemetry: bool) -> bool {
        let mut pending = self
            .state
            .pending
            .lock()
            .expect("Host event queue was poisoned");
        if pending.closed {
            return false;
        }
        if telemetry
            && let Some(queued) = pending
                .frames
                .iter_mut()
                .find(|queued| queued.event == frame.event)
        {
            *queued = frame;
            return true;
        }
        if pending.frames.len() >= HOST_EVENT_QUEUE_CAPACITY {
            let victim = pending
                .frames
                .iter()
                .position(|queued| is_telemetry_frame(&queued.event));
            match victim {
                Some(victim) => {
                    pending.frames.remove(victim);
                }
                None if telemetry => return true,
                None => {
                    tracing::warn!(
                        event = %frame.event,
                        "local Host event subscriber fell behind; closing its connection"
                    );
                    drop(pending);
                    self.close();
                    return false;
                }
            }
        }
        pending.frames.push_back(frame);
        drop(pending);
        self.state.available.notify_one();
        true
    }

    /// Marks the queue closed so blocked readers finish and the hub drops it.
    fn close(&self) {
        let mut pending = self
            .state
            .pending
            .lock()
            .expect("Host event queue was poisoned");
        pending.closed = true;
        pending.frames.clear();
        self.state.available.notify_all();
    }
}

impl EventQueueState {
    fn recv(&self) -> Result<HostEventFrame, mpsc::RecvError> {
        let mut pending = self.pending.lock().expect("Host event queue was poisoned");
        loop {
            if let Some(frame) = pending.frames.pop_front() {
                return Ok(frame);
            }
            if pending.closed {
                return Err(mpsc::RecvError);
            }
            pending = self
                .available
                .wait(pending)
                .expect("Host event queue was poisoned");
        }
    }

    fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<HostEventFrame, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut pending = self.pending.lock().expect("Host event queue was poisoned");
        loop {
            if let Some(frame) = pending.frames.pop_front() {
                return Ok(frame);
            }
            if pending.closed {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
            let (next, _timeout_result) = self
                .available
                .wait_timeout(pending, remaining)
                .expect("Host event queue was poisoned");
            pending = next;
        }
    }

    fn try_recv(&self) -> Result<HostEventFrame, mpsc::TryRecvError> {
        let mut pending = self.pending.lock().expect("Host event queue was poisoned");
        if let Some(frame) = pending.frames.pop_front() {
            return Ok(frame);
        }
        if pending.closed {
            return Err(mpsc::TryRecvError::Disconnected);
        }
        Err(mpsc::TryRecvError::Empty)
    }
}

enum HostEventSubscriber {
    Events(EventQueue),
    PluginPersistence(Sender<HostEventFrame>),
}

/// Fans Host events out to the shell and to independent local event clients.
pub struct HostEventHub {
    shell: SharedHostEventSink,
    emit_gate: Mutex<()>,
    subscribers: Mutex<Vec<HostEventSubscriber>>,
    closed: std::sync::atomic::AtomicBool,
}

impl HostEventHub {
    /// Creates a hub using the supplied shell sink as its first consumer.
    pub fn new(shell: SharedHostEventSink) -> Arc<Self> {
        Arc::new(Self {
            shell,
            emit_gate: Mutex::new(()),
            subscribers: Mutex::new(Vec::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Subscribes one local event consumer.
    pub fn subscribe(&self) -> Option<HostEventSubscription> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let queue = EventQueue::new();
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("Host event subscribers lock was poisoned");
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        subscribers.push(HostEventSubscriber::Events(queue.clone()));
        Some(HostEventSubscription {
            inner: SubscriptionInner::Events(queue),
        })
    }

    pub(crate) fn subscribe_plugin_persistence(&self) -> Option<HostEventSubscription> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let (sender, receiver) = mpsc::channel();
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("Host event subscribers lock was poisoned");
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        subscribers.push(HostEventSubscriber::PluginPersistence(sender));
        Some(HostEventSubscription {
            inner: SubscriptionInner::PluginPersistence(receiver),
        })
    }

    /// Closes all event subscriptions during orderly Host shutdown.
    pub fn close(&self) {
        let _emit_gate = self
            .emit_gate
            .lock()
            .expect("Host event emit gate was poisoned");
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.subscribers
            .lock()
            .expect("Host event subscribers lock was poisoned")
            .drain(..)
            .for_each(|subscriber| {
                if let HostEventSubscriber::Events(queue) = subscriber {
                    queue.close();
                }
            });
    }
}

impl Drop for HostEventHub {
    fn drop(&mut self) {
        self.close();
    }
}

impl HostEventSink for HostEventHub {
    fn emit(&self, event: HostEvent) {
        let _emit_gate = self
            .emit_gate
            .lock()
            .expect("Host event emit gate was poisoned");
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        self.shell.emit(event.clone());
        let frame = match event.to_control_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(error = %error, "local Host event could not be serialized");
                return;
            }
        };
        let telemetry = event.is_coalescible();
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("Host event subscribers lock was poisoned");
        subscribers.retain(|subscriber| match subscriber {
            HostEventSubscriber::Events(queue) => queue.push(frame.clone(), telemetry),
            HostEventSubscriber::PluginPersistence(subscriber) => {
                if is_plugin_persistence_event(&frame.event) {
                    subscriber.send(frame.clone()).is_ok()
                } else {
                    true
                }
            }
        });
    }
}

fn is_plugin_persistence_event(event: &str) -> bool {
    matches!(
        event,
        "runtime-restarted" | "track-plugin-state-changed" | "track-plugin-parameter-changed"
    )
}

/// A sequence of Host event frames delivered to one local consumer.
pub struct HostEventSubscription {
    inner: SubscriptionInner,
}

enum SubscriptionInner {
    Events(EventQueue),
    PluginPersistence(Receiver<HostEventFrame>),
}

impl HostEventSubscription {
    /// Waits for the next event or for the Host to close the subscription.
    pub fn recv(&self) -> Result<HostEventFrame, mpsc::RecvError> {
        match &self.inner {
            SubscriptionInner::Events(queue) => queue.state.recv(),
            SubscriptionInner::PluginPersistence(receiver) => receiver.recv(),
        }
    }

    /// Waits for the next event for at most `timeout`.
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<HostEventFrame, mpsc::RecvTimeoutError> {
        match &self.inner {
            SubscriptionInner::Events(queue) => queue.state.recv_timeout(timeout),
            SubscriptionInner::PluginPersistence(receiver) => receiver.recv_timeout(timeout),
        }
    }

    /// Drains one already queued event without waiting.
    pub fn try_recv(&self) -> Result<HostEventFrame, mpsc::TryRecvError> {
        match &self.inner {
            SubscriptionInner::Events(queue) => queue.state.try_recv(),
            SubscriptionInner::PluginPersistence(receiver) => receiver.try_recv(),
        }
    }
}

impl Drop for HostEventSubscription {
    fn drop(&mut self) {
        if let SubscriptionInner::Events(queue) = &self.inner {
            queue.close();
        }
    }
}

/// A small event sink useful to tests and embedding applications.
#[derive(Debug, Default)]
pub struct RecordingHostEventSink {
    events: std::sync::Mutex<Vec<HostEvent>>,
}

impl RecordingHostEventSink {
    /// Returns a snapshot of events observed so far.
    pub fn events(&self) -> Vec<HostEvent> {
        self.events
            .lock()
            .expect("host event sink lock poisoned")
            .clone()
    }
}

impl HostEventSink for RecordingHostEventSink {
    fn emit(&self, event: HostEvent) {
        self.events
            .lock()
            .expect("host event sink lock poisoned")
            .push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_hub_fans_out_shell_and_local_frames() {
        let shell = Arc::new(RecordingHostEventSink::default());
        let hub = HostEventHub::new(shell.clone());
        let subscription = hub.subscribe().unwrap();

        hub.emit(HostEvent::RuntimeRestarted { generation: 4 });

        let frame = subscription.recv().unwrap();
        assert_eq!(frame.event, "runtime-restarted");
        assert_eq!(frame.payload["generation"], 4);
        let shell_events = shell.events();
        assert_eq!(shell_events.len(), 1);
        assert!(matches!(
            &shell_events[0],
            HostEvent::RuntimeRestarted { generation: 4 }
        ));
    }

    #[test]
    fn a_full_critical_queue_drops_incoming_telemetry_without_disconnect() {
        let shell = Arc::new(NoopHostEventSink);
        let hub = HostEventHub::new(shell);
        let subscription = hub.subscribe().unwrap();

        for generation in 0..HOST_EVENT_QUEUE_CAPACITY as u64 {
            hub.emit(HostEvent::RuntimeRestarted { generation });
        }
        hub.emit(HostEvent::AudioMeters(serde_json::json!({ "tick": 999 })));

        let mut received = Vec::new();
        while let Ok(frame) = subscription.try_recv() {
            received.push(frame.payload["generation"].as_u64().unwrap());
        }
        assert_eq!(received.len(), HOST_EVENT_QUEUE_CAPACITY);
        assert_eq!(received.first().copied(), Some(0));
        assert_eq!(
            received.last().copied(),
            Some(HOST_EVENT_QUEUE_CAPACITY as u64 - 1)
        );

        // Dropping telemetry from a full critical queue does not disconnect
        // the subscriber.
        hub.emit(HostEvent::RuntimeRestarted { generation: 999 });
        assert_eq!(subscription.try_recv().unwrap().payload["generation"], 999);
    }

    #[test]
    fn a_full_critical_queue_closes_on_incoming_critical_event() {
        let shell = Arc::new(NoopHostEventSink);
        let hub = HostEventHub::new(shell);
        let subscription = hub.subscribe().unwrap();

        for generation in 0..HOST_EVENT_QUEUE_CAPACITY as u64 {
            hub.emit(HostEvent::RuntimeRestarted { generation });
        }
        hub.emit(HostEvent::RuntimeRestarted {
            generation: HOST_EVENT_QUEUE_CAPACITY as u64,
        });

        assert!(matches!(
            subscription.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn a_telemetry_flood_coalesces_and_preserves_critical_delivery() {
        let shell = Arc::new(NoopHostEventSink);
        let hub = HostEventHub::new(shell);
        let subscription = hub.subscribe().unwrap();

        for tick in 0..(HOST_EVENT_QUEUE_CAPACITY as u64 * 2) {
            hub.emit(HostEvent::TransportStatus(
                serde_json::json!({ "tick": tick }),
            ));
        }
        hub.emit(HostEvent::RuntimeRestarted { generation: 7 });
        hub.emit(HostEvent::TransportStatus(
            serde_json::json!({ "tick": 10_000 }),
        ));

        let mut frames = Vec::new();
        while let Ok(frame) = subscription.try_recv() {
            frames.push(frame);
        }
        // Latest-wins telemetry keeps one slot, and the critical event is
        // never dropped by the flood.
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].event, "transport-status");
        assert_eq!(frames[0].payload["tick"], 10_000);
        assert_eq!(frames[1].event, "runtime-restarted");
    }

    #[test]
    fn closing_the_hub_finishes_blocked_readers() {
        let shell = Arc::new(NoopHostEventSink);
        let hub = HostEventHub::new(shell);
        let subscription = hub.subscribe().unwrap();
        let reader = std::thread::spawn(move || subscription.recv());

        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(hub);

        assert!(reader.join().unwrap().is_err());
    }
}
