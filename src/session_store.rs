use std::fs::File;

use crate::session::Session;
use crate::state_file::StateFile;

pub struct SessionStore {
    file: StateFile,
}

impl SessionStore {
    pub fn new(file: StateFile) -> Self {
        Self { file }
    }

    // Held across a whole press, and by the track hook while it decides whether
    // a focus event is the cycle's own, so neither sees half a press.
    pub fn lock(&self) -> anyhow::Result<File> {
        Ok(self.file.lock()?)
    }

    pub fn load(&self) -> Option<Session> {
        serde_json::from_str(&self.file.read()?).ok()
    }

    pub fn save(&self, session: &Session) -> anyhow::Result<()> {
        Ok(self.file.write(&serde_json::to_string(session)?)?)
    }

    pub fn clear(&self) {
        self.file.remove();
    }

    pub fn mark_released(&self, started_at: u64, now: u64) -> anyhow::Result<()> {
        let _turn = self.lock()?;
        match self.load() {
            Some(session) if session.started_at() == started_at => {
                self.save(&session.released(now))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::session_holding_alt;
    use tempfile::TempDir;

    fn store_in(dir: &TempDir) -> SessionStore {
        SessionStore::new(StateFile::at(dir.path().join("session.json")))
    }

    #[test]
    fn a_saved_session_loads_back() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save(&session_holding_alt(5000)).unwrap();

        let loaded = store.load();

        assert_eq!(loaded.unwrap().target(), "B");
    }

    #[test]
    fn a_cleared_session_is_gone() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save(&session_holding_alt(5000)).unwrap();
        store.clear();

        let loaded = store.load();

        assert!(loaded.is_none());
    }

    #[test]
    fn releasing_ends_the_cycle_the_watcher_was_started_for() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save(&session_holding_alt(5000)).unwrap();
        store.mark_released(5000, 5100).unwrap();

        let session = store.load().unwrap();

        assert!(!session.is_active(5200));
    }

    #[test]
    fn a_watcher_for_an_earlier_cycle_leaves_the_current_one_running() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save(&session_holding_alt(5000)).unwrap();
        store.mark_released(4000, 5100).unwrap();

        let session = store.load().unwrap();

        assert!(session.is_active(5200));
    }
}
