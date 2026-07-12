use crate::model::{RecoveryCandidate, ScratchSession};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const GENERATIONS_TO_KEEP: usize = 20;

#[derive(Debug)]
pub struct SessionStore {
    scratch_dir: PathBuf,
    generations_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_root: &Path) -> Self {
        let scratch_dir = data_root.join("scratch");
        let generations_dir = scratch_dir.join("generations");
        Self {
            scratch_dir,
            generations_dir,
        }
    }

    pub fn ensure_layout(&self) -> io::Result<()> {
        fs::create_dir_all(&self.generations_dir)
    }

    pub fn load_or_create(&self) -> io::Result<(ScratchSession, bool)> {
        self.ensure_layout()?;
        let current = self.scratch_dir.join("current.json");
        if let Ok(session) = read_session(&current) {
            return Ok((session, false));
        }

        for candidate in self.generation_files_newest_first()? {
            if let Ok(session) = read_session(&candidate) {
                return Ok((session, true));
            }
        }

        let session = ScratchSession::new(now_ms());
        self.save(&session)?;
        Ok((session, false))
    }

    pub fn save(&self, session: &ScratchSession) -> io::Result<()> {
        self.ensure_layout()?;
        let current = self.scratch_dir.join("current.json");
        if current.is_file() {
            let generation =
                self.generations_dir
                    .join(format!("{}-{}.json", now_ms(), std::process::id()));
            fs::copy(&current, generation)?;
        }

        let payload = serde_json::to_vec_pretty(session)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary =
            self.scratch_dir
                .join(format!(".current-{}-{}.tmp", std::process::id(), now_ms()));
        {
            let mut file = File::create(&temporary)?;
            file.write_all(&payload)?;
            file.sync_all()?;
        }

        if let Err(error) = fs::rename(&temporary, &current) {
            if current.exists() {
                fs::remove_file(&current)?;
                fs::rename(&temporary, &current)?;
            } else {
                return Err(error);
            }
        }
        self.prune_generations()
    }

    pub fn recovery_candidates(&self) -> io::Result<Vec<RecoveryCandidate>> {
        self.ensure_layout()?;
        let mut candidates = Vec::new();
        for path in self.generation_files_newest_first()? {
            let Ok(session) = read_session(&path) else {
                continue;
            };
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            candidates.push(RecoveryCandidate {
                file_name: file_name.to_owned(),
                updated_at_ms: session.updated_at_ms,
                session_id: session.session_id,
                project_name: session.project_name,
                note: session.note,
            });
        }
        Ok(candidates)
    }

    pub fn restore_generation(&self, file_name: &str) -> io::Result<ScratchSession> {
        if file_name.trim().is_empty()
            || file_name
                != Path::new(file_name)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
            || !file_name.ends_with(".json")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Recovery generation name is invalid.",
            ));
        }
        let path = self.generations_dir.join(file_name);
        let session = read_session(&path)?;
        self.save(&session)?;
        Ok(session)
    }

    fn generation_files_newest_first(&self) -> io::Result<Vec<PathBuf>> {
        let mut files = fs::read_dir(&self.generations_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        Ok(files)
    }

    fn prune_generations(&self) -> io::Result<()> {
        for stale in self
            .generation_files_newest_first()?
            .into_iter()
            .skip(GENERATIONS_TO_KEEP)
        {
            let _ = fs::remove_file(stale);
        }
        Ok(())
    }
}

fn read_session(path: &Path) -> io::Result<ScratchSession> {
    let payload = fs::read(path)?;
    serde_json::from_slice::<ScratchSession>(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .validate_and_normalize()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AiChangeSet, DeviceKind, RackDevice, SamplePad, SessionSnapshot, TimelineClip,
    };

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-{name}-{}", now_ms()))
    }

    #[test]
    fn keeps_last_valid_generation_when_current_is_corrupt() {
        let root = test_root("recovery");
        let store = SessionStore::new(&root);
        let mut first = ScratchSession::new(now_ms());
        first.note = "recover me".into();
        store.save(&first).unwrap();
        let mut second = first.clone();
        second.note = "newer".into();
        store.save(&second).unwrap();
        fs::write(root.join("scratch/current.json"), b"not json").unwrap();

        let (recovered, used_generation) = store.load_or_create().unwrap();
        assert!(used_generation);
        assert_eq!(recovered.note, "recover me");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_and_restores_valid_recovery_generations() {
        let root = test_root("recovery-candidates");
        let store = SessionStore::new(&root);
        let mut first = ScratchSession::new(now_ms());
        first.note = "stable choice".into();
        store.save(&first).unwrap();
        let mut second = first.clone();
        second.note = "newer choice".into();
        store.save(&second).unwrap();
        let candidates = store.recovery_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        let restored = store.restore_generation(&candidates[0].file_name).unwrap();
        assert_eq!(restored.note, "stable choice");
        let (current, recovered) = store.load_or_create().unwrap();
        assert!(!recovered);
        assert_eq!(current.note, "stable choice");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_recovery_paths_outside_generation_directory() {
        let root = test_root("recovery-path");
        let store = SessionStore::new(&root);
        let error = store.restore_generation("..\\current.json").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clamps_unsafe_master_gain_before_persisting() {
        let mut session = ScratchSession::new(now_ms());
        session.master_db = 12.0;
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.master_db, 0.0);
    }

    #[test]
    fn preserves_audio_driver_preference() {
        let mut session = ScratchSession::new(now_ms());
        session.audio_driver = Some("ASIO".into());
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.audio_driver.as_deref(), Some("ASIO"));
    }

    #[test]
    fn preserves_ai_permission_and_context_preferences() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_permission = "Apply".into();
        session.ai_context = vec!["selectedRack".into(), "analysis".into()];
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.ai_permission, "Apply");
        assert_eq!(normalized.ai_context, vec!["selectedRack", "analysis"]);
    }

    #[test]
    fn rejects_unknown_ai_permission_and_context() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_permission = "Auto".into();
        assert!(session.validate_and_normalize().is_err());

        let mut session = ScratchSession::new(now_ms());
        session.ai_context = vec!["unknown".into(), "analysis".into(), "analysis".into()];
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.ai_context, vec!["analysis"]);
    }

    #[test]
    fn preserves_reversible_ai_change_set_history() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_history.push(AiChangeSet {
            id: "ai:1".into(),
            created_at_ms: now_ms(),
            permission: "Apply".into(),
            target: "clip:1".into(),
            current_gain_db: 0.0,
            proposed_gain_db: -3.0,
            reason: "Match reference RMS".into(),
            expected_effect: "Closer perceived level".into(),
            risk: "Low · reversible".into(),
            context: vec!["analysis".into(), "selectedClip".into()],
            applied: true,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.ai_history.len(), 1);
        assert_eq!(decoded.ai_history[0].target, "clip:1");
        assert!(decoded.ai_history[0].applied);
    }

    #[test]
    fn preserves_persisted_plugin_path() {
        let mut session = ScratchSession::new(now_ms());
        session.rack.push(RackDevice {
            id: "plugin:amplitube".into(),
            name: "AmpliTube 5".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\Program Files\Common Files\VST3\AmpliTube 5.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded
                .rack
                .last()
                .and_then(|device| device.path.as_deref()),
            Some(r"C:\Program Files\Common Files\VST3\AmpliTube 5.vst3")
        );
    }

    #[test]
    fn preserves_a_b_snapshot_state() {
        let mut session = ScratchSession::new(now_ms());
        session.snapshots.push(SessionSnapshot {
            id: "snapshot:A".into(),
            name: "A".into(),
            created_at_ms: now_ms(),
            description: "Clean reference".into(),
            tag: Some("reference".into()),
            parent_id: None,
            master_db: -18.0,
            rack: session.rack.clone(),
            macros: session.macros.clone(),
        });
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.snapshots[0].name, "A");
        assert_eq!(normalized.snapshots[0].rack.len(), 3);
    }

    #[test]
    fn preserves_non_destructive_timeline_clip() {
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(TimelineClip {
            id: "clip:take-1".into(),
            asset_path: r"C:\recordings\take-1\processed.wav".into(),
            name: "take-1".into(),
            track_id: "main".into(),
            start_ms: 250,
            duration_ms: 1_000,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.timeline[0].start_ms, 250);
        assert_eq!(
            decoded.timeline[0].asset_path,
            r"C:\recordings\take-1\processed.wav"
        );
    }

    #[test]
    fn preserves_sample_pad_mapping() {
        let mut session = ScratchSession::new(now_ms());
        session.sample_pads.push(SamplePad {
            id: "pad:take-1".into(),
            name: "take-1".into(),
            asset_path: r"C:\recordings\take-1\processed.wav".into(),
            start_ms: 0,
            end_ms: 1_000,
            midi_key: 36,
            gain_db: 0.0,
            loop_enabled: false,
        });
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.sample_pads[0].midi_key, 36);
        assert_eq!(normalized.sample_pads[0].end_ms, 1_000);
    }
}
