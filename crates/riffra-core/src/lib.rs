//! Platform-independent production domain and application state for Riffra.
//!
//! This crate contains the canonical production models and the state shared by
//! desktop and headless application hosts. It deliberately contains no Tauri, WebView,
//! process-management, audio-device, or operating-system integration.

mod app;
pub mod application;
pub mod domain;
mod errors;
pub mod ports;

pub use app::{AppCore, CanonicalSnapshot, CanonicalState, HistoryState, PreparedSession};
pub use domain::*;
pub use errors::{ApplicationError, DomainError, InputLocation};
pub use ports::{PortError, ProjectionKey, SessionStorage};

impl AppCore {
    /// Creates an application facade over the canonical Core state.
    pub fn application<'a, S: SessionStorage + ?Sized>(
        &'a mut self,
        storage: &'a S,
    ) -> application::Application<'a, S> {
        application::Application::new(self, storage)
    }
}
