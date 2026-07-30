//! Platform-independent production domain and application state for Riffra.
//!
//! This crate contains the canonical production models and the state shared by
//! desktop and headless application hosts. It deliberately contains no Tauri, WebView,
//! process-management, audio-device, or operating-system integration.

mod app;
pub mod asset;
mod errors;
pub mod rack;
mod runtime;
pub mod session;

pub use app::AppCore;
pub use errors::DomainError;
pub use runtime::{AudioRuntime, OfflineRenderRequest};
