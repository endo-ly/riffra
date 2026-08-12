//! Platform-independent production domain and application state for Riffra.
//!
//! This crate contains the canonical production models and the state shared by
//! desktop and headless application hosts. It deliberately contains no Tauri, WebView,
//! process-management, audio-device, or operating-system integration.

mod app;
pub mod application;
pub mod asset;
mod errors;
mod history;
pub mod ports;
pub mod rack;
mod runtime;
pub mod session;

pub use app::{AppCore, CanonicalSessionHandle, CanonicalSnapshot, HistoryState};
pub use errors::{ApplicationError, DomainError};
pub use ports::{PortError, RuntimeProjection, RuntimeProjectionRequest, SessionStorage};
pub use runtime::{AudioRuntime, OfflineRenderRequest};

impl<A> AppCore<A> {
    /// Creates an application facade over the canonical Core state.
    pub fn application<'a, S: SessionStorage + ?Sized>(
        &'a self,
        storage: &'a S,
    ) -> application::Application<'a, A, S> {
        application::Application::new(self, storage)
    }
}
