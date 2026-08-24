use super::{
    RecordingAsset, RecordingContext, record_another_take, start_recording, stop_recording,
};
use crate::model::{AudioStatus, RecordingStopResult};
use crate::{AudioSupervisor, RuntimeReconciler};
use riffra_core::AppCore;
use std::path::Path;

/// Shared recording workflow used by Tauri commands and Host control.
pub struct RecordingService<'a> {
    /// Canonical application state being recorded.
    pub core: &'a AppCore<AudioSupervisor>,
    /// Native audio runtime receiving the capture commands.
    pub audio: &'a AudioSupervisor,
    /// Runtime graph that must be active before capture starts.
    pub runtime: &'a RuntimeReconciler<AudioSupervisor>,
    /// Host Data Root containing the recording Inbox.
    pub data_root: &'a Path,
    /// Whether external recording is disabled for this Host.
    pub safe_mode: bool,
}

impl RecordingService<'_> {
    /// Starts a new recording take and persists its capture manifest.
    pub fn start(&self, recording_session_id: Option<&str>) -> Result<AudioStatus, String> {
        let context = self.context();
        match recording_session_id {
            Some(id) => record_another_take(&context, id),
            None => start_recording(&context),
        }
    }

    /// Stops the native capture, finalizes its products, and returns the
    /// canonical state visible after the stop.
    pub fn stop(&self) -> Result<RecordingStopResult, String> {
        stop_recording(&self.context())
    }

    /// Returns the latest native recording status.
    pub fn status(&self) -> Result<AudioStatus, String> {
        self.audio
            .refresh_status()
            .map_err(|error| error.to_string())
    }

    /// Lists Inbox recording products through the shared repository.
    pub fn list(&self, query: Option<&str>) -> Result<Vec<RecordingAsset>, String> {
        super::list(self.data_root, query)
    }

    /// Renames one Inbox recording through the shared repository workflow.
    pub fn rename_recording(&self, id: &str, new_name: &str) -> Result<String, String> {
        super::rename_recording(&self.context(), id, new_name)
    }

    /// Moves one Inbox recording to the archive area.
    pub fn archive_recording(&self, id: &str) -> Result<String, String> {
        super::archive_recording(&self.context(), id)
    }

    /// Promotes one Inbox recording into the managed library area.
    pub fn promote_recording(&self, id: &str) -> Result<String, String> {
        super::promote_recording(&self.context(), id)
    }

    /// Updates recording metadata in the shared library model.
    pub fn tag_recording(
        &self,
        id: &str,
        tag: Option<String>,
        note: Option<String>,
    ) -> Result<crate::library::LibraryAsset, String> {
        super::tag_recording(&self.context(), id, tag, note)
    }

    /// Deletes one Inbox recording through the shared repository workflow.
    pub fn delete_recording(&self, id: &str) -> Result<(), String> {
        super::delete_recording(&self.context(), id)
    }

    /// Finds duplicate Inbox recordings using the shared repository model.
    pub fn detect_duplicate_recordings(&self) -> Result<Vec<Vec<String>>, String> {
        super::detect_duplicate_recordings(&self.context())
    }

    fn context(&self) -> RecordingContext<'_> {
        RecordingContext {
            core: self.core,
            audio: self.audio,
            runtime: self.runtime,
            data_root: self.data_root,
            safe_mode: self.safe_mode,
        }
    }
}
