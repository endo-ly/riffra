use crate::protocol::{Command, CommandResult};
use crate::storage::SessionFileStorage;
use riffra_core::AppCore;

pub struct Dispatcher {
    core: AppCore<()>,
    storage: SessionFileStorage,
}

impl Dispatcher {
    pub fn open(storage: SessionFileStorage) -> Result<Self, String> {
        let session = storage.load_or_new()?;
        let data_root = storage
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        Ok(Self {
            core: AppCore::new(data_root, session, (), false, false),
            storage,
        })
    }

    pub fn dispatch(&self, command: Command) -> Result<CommandResult, String> {
        let application = self.core.application(&self.storage);
        match command {
            Command::GetSession => application
                .get_session()
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
            Command::ListTracks => application
                .list_tracks()
                .map(CommandResult::Tracks)
                .map_err(|error| error.to_string()),
            Command::AddTrack { name, kind } => application
                .add_track(name, kind)
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
            Command::RemoveTrack { track_id } => application
                .remove_track(&track_id)
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
            Command::UpdateSessionSettings { patch } => application
                .update_session_settings(patch)
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
            Command::Undo => application
                .undo()
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
            Command::Redo => application
                .redo()
                .map(|session| CommandResult::Session(Box::new(session)))
                .map_err(|error| error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::CommandResult;
    use riffra_core::TrackKind;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn session_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("riffra-cli-{name}-{nonce}"))
            .join("session.json")
    }

    fn track_count(result: CommandResult) -> usize {
        match result {
            CommandResult::Session(session) => session.arrangement.tracks.len(),
            CommandResult::Tracks(tracks) => tracks.len(),
        }
    }

    #[test]
    fn track_edit_is_persisted_and_visible_after_reopen() {
        let path = session_path("persist");
        let dispatcher = Dispatcher::open(SessionFileStorage::new(path.clone())).unwrap();

        dispatcher
            .dispatch(Command::AddTrack {
                name: "Bass".into(),
                kind: TrackKind::Instrument,
            })
            .unwrap();
        let reopened = Dispatcher::open(SessionFileStorage::new(path.clone())).unwrap();

        assert_eq!(
            track_count(reopened.dispatch(Command::GetSession).unwrap()),
            1
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn undo_and_redo_use_core_history() {
        let path = session_path("history");
        let dispatcher = Dispatcher::open(SessionFileStorage::new(path.clone())).unwrap();
        dispatcher
            .dispatch(Command::AddTrack {
                name: "Bass".into(),
                kind: TrackKind::Instrument,
            })
            .unwrap();

        assert_eq!(track_count(dispatcher.dispatch(Command::Undo).unwrap()), 0);
        assert_eq!(track_count(dispatcher.dispatch(Command::Redo).unwrap()), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejected_command_does_not_change_the_session_file() {
        let path = session_path("rejected");
        let dispatcher = Dispatcher::open(SessionFileStorage::new(path.clone())).unwrap();
        dispatcher
            .dispatch(Command::AddTrack {
                name: "Audio".into(),
                kind: TrackKind::Audio,
            })
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        let result = dispatcher.dispatch(Command::RemoveTrack {
            track_id: "track:missing".into(),
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_session_is_not_overwritten() {
        let path = session_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json").unwrap();

        let result = Dispatcher::open(SessionFileStorage::new(path.clone()));

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
