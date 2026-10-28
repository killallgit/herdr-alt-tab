use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::command::Command as Invocation;
use crate::keyboard::Keyboard;
use crate::session::MAX_HOLD_MILLIS;
use crate::session_store::SessionStore;

const POLL_INTERVAL: Duration = Duration::from_millis(15);

pub trait ReleaseWatcher {
    fn watch(&self, started_at: u64) -> anyhow::Result<()>;
}

// Outlives the action that started it, which exits as soon as it has focused a pane.
pub struct DetachedWatcher;

impl ReleaseWatcher for DetachedWatcher {
    fn watch(&self, started_at: u64) -> anyhow::Result<()> {
        Command::new(std::env::current_exe()?)
            .args(Invocation::Watch { started_at }.args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(())
    }
}

pub struct ReleaseWaiter<'a> {
    pub keyboard: &'a dyn Keyboard,
    pub sessions: &'a SessionStore,
    pub clock: &'a dyn Clock,
}

impl ReleaseWaiter<'_> {
    pub fn wait(&self, started_at: u64) -> anyhow::Result<()> {
        let give_up_at = Instant::now() + Duration::from_millis(MAX_HOLD_MILLIS);
        while self.keyboard.alt_held() == Some(true) && Instant::now() < give_up_at {
            std::thread::sleep(POLL_INTERVAL);
        }
        self.sessions
            .mark_released(started_at, self.clock.now_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_file::StateFile;
    use crate::test_support::{FakeClock, FakeKeyboard, session_holding_alt};
    use tempfile::TempDir;

    fn store_with_cycle_started_at(dir: &TempDir, started_at: u64) -> SessionStore {
        let store = SessionStore::new(StateFile::at(dir.path().join("session.json")));
        store.save(&session_holding_alt(started_at)).unwrap();
        store
    }

    fn wait(keyboard: &FakeKeyboard, store: &SessionStore, started_at: u64) {
        let clock = FakeClock::at(5100);
        ReleaseWaiter {
            keyboard,
            sessions: store,
            clock: &clock,
        }
        .wait(started_at)
        .unwrap();
    }

    #[test]
    fn the_cycle_ends_once_alt_comes_up() {
        let dir = TempDir::new().unwrap();
        let store = store_with_cycle_started_at(&dir, 5000);
        wait(
            &FakeKeyboard::reading(&[Some(true), Some(true), Some(false)]),
            &store,
            5000,
        );

        let session = store.load().unwrap();

        assert!(!session.is_active(5200));
    }

    #[test]
    fn a_keyboard_that_cannot_report_alt_ends_the_cycle_at_once() {
        let dir = TempDir::new().unwrap();
        let store = store_with_cycle_started_at(&dir, 5000);
        wait(&FakeKeyboard::reading(&[None]), &store, 5000);

        let session = store.load().unwrap();

        assert!(!session.is_active(5200));
    }

    #[test]
    fn a_watcher_for_an_earlier_cycle_leaves_the_current_one_running() {
        let dir = TempDir::new().unwrap();
        let store = store_with_cycle_started_at(&dir, 5000);
        wait(&FakeKeyboard::reading(&[Some(false)]), &store, 4000);

        let session = store.load().unwrap();

        assert!(session.is_active(5200));
    }
}
