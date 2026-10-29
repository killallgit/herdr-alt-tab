use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::path::PathBuf;

use fs2::FileExt;
use tempfile::TempDir;

use crate::clock::Clock;
use crate::connection::Connection;
use crate::cycle::{Collaborators, Cycle, CycleId, Direction};
use crate::herdr::{Herdr, HerdrError, Pane, Panes};
use crate::history::History;
use crate::keyboard::Keyboard;
use crate::release::ReleaseWatcher;

pub struct FakeClock(Cell<u64>);

impl FakeClock {
    pub fn at(millis: u64) -> Self {
        Self(Cell::new(millis))
    }
}

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        self.0.get()
    }

    fn pause(&self, millis: u64) {
        self.0.set(self.0.get() + millis);
    }
}

pub struct FakeHerdr {
    panes: RefCell<Vec<String>>,
    focused: RefCell<Option<String>>,
    focus_requests: RefCell<Vec<String>>,
    focus_fails: Cell<bool>,
    lock_probe: RefCell<Option<PathBuf>>,
    lock_held_when_asked: RefCell<Vec<bool>>,
}

impl FakeHerdr {
    fn with_panes(ids: &[&str]) -> Self {
        Self {
            panes: RefCell::new(ids.iter().map(|id| id.to_string()).collect()),
            focused: RefCell::new(ids.first().map(|id| id.to_string())),
            focus_requests: RefCell::new(Vec::new()),
            focus_fails: Cell::new(false),
            lock_probe: RefCell::new(None),
            lock_held_when_asked: RefCell::new(Vec::new()),
        }
    }

    pub fn open(&self, pane_id: &str) {
        self.panes.borrow_mut().push(pane_id.to_string());
    }

    pub fn fail_focus(&self) {
        self.focus_fails.set(true);
    }

    pub fn allow_focus(&self) {
        self.focus_fails.set(false);
    }

    pub fn focus_requests(&self) -> Vec<String> {
        self.focus_requests.borrow().clone()
    }

    fn note_whether_locked(&self) {
        if let Some(path) = self.lock_probe.borrow().as_ref() {
            let rival = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)
                .unwrap();
            self.lock_held_when_asked
                .borrow_mut()
                .push(rival.try_lock_exclusive().is_err());
        }
    }
}

impl Herdr for FakeHerdr {
    fn panes(&self) -> Result<Panes, HerdrError> {
        self.note_whether_locked();
        let focused = self.focused.borrow();
        Ok(Panes::new(
            self.panes
                .borrow()
                .iter()
                .map(|id| Pane {
                    id: id.clone(),
                    focused: focused.as_deref() == Some(id),
                })
                .collect(),
        ))
    }

    fn focus(&self, pane_id: &str) -> Result<(), HerdrError> {
        if self.focus_fails.get() {
            return Err(HerdrError::Rejected {
                request: "pane.focus".into(),
                reason: "refused".into(),
            });
        }
        self.focus_requests.borrow_mut().push(pane_id.to_string());
        *self.focused.borrow_mut() = Some(pane_id.to_string());
        Ok(())
    }
}

/// Answers each `alt_held` call from `readings` in turn, then keeps answering `resting`.
pub struct FakeKeyboard {
    readings: RefCell<VecDeque<Option<bool>>>,
    resting: Cell<Option<bool>>,
}

impl FakeKeyboard {
    pub fn reading(readings: &[Option<bool>]) -> Self {
        Self {
            readings: RefCell::new(readings.iter().copied().collect()),
            resting: Cell::new(Some(false)),
        }
    }

    pub fn set_alt(&self, held: Option<bool>) {
        self.resting.set(held);
    }
}

impl Keyboard for FakeKeyboard {
    fn alt_held(&self) -> Option<bool> {
        self.readings
            .borrow_mut()
            .pop_front()
            .unwrap_or(self.resting.get())
    }
}

pub struct FakeWatcher(RefCell<Vec<CycleId>>);

impl FakeWatcher {
    pub fn started(&self) -> Vec<CycleId> {
        self.0.borrow().clone()
    }
}

impl ReleaseWatcher for FakeWatcher {
    fn watch(&self, cycle: CycleId) -> anyhow::Result<()> {
        self.0.borrow_mut().push(cycle);
        Ok(())
    }
}

/// A Herdr with the given panes, most recently used first, focused on the
/// first, and Alt held down.
pub struct Rig {
    pub herdr: FakeHerdr,
    pub keyboard: FakeKeyboard,
    pub watcher: FakeWatcher,
    pub history: History,
    pub clock: FakeClock,
    connection: Connection,
    _state_dir: TempDir,
}

impl Rig {
    pub fn with_panes(panes: &[&str]) -> Self {
        let state_dir = TempDir::new().unwrap();
        let connection = Connection::new(state_dir.path().to_path_buf(), None);
        let history = History::new(connection.state_file("history"), connection.lock("history"));
        for (age, pane_id) in panes.iter().rev().enumerate() {
            history.record(pane_id, 100 * (age as u64 + 1)).unwrap();
        }
        let keyboard = FakeKeyboard::reading(&[]);
        keyboard.set_alt(Some(true));
        Self {
            herdr: FakeHerdr::with_panes(panes),
            keyboard,
            watcher: FakeWatcher(RefCell::new(Vec::new())),
            history,
            clock: FakeClock::at(10_000),
            connection,
            _state_dir: state_dir,
        }
    }

    pub fn cycle(&self) -> Cycle<'_> {
        Cycle::new(
            &self.connection,
            Collaborators {
                herdr: &self.herdr,
                keyboard: &self.keyboard,
                watcher: &self.watcher,
                history: &self.history,
                clock: &self.clock,
            },
        )
    }

    pub fn press(&self, direction: Direction) -> anyhow::Result<()> {
        self.cycle().press(direction)
    }

    pub fn pass_time(&self, millis: u64) {
        self.clock.pause(millis);
    }

    pub fn user_focuses(&self, pane_id: &str) {
        *self.herdr.focused.borrow_mut() = Some(pane_id.to_string());
    }

    /// Plays the release watcher of the cycle that last started one.
    pub fn release_alt(&self) {
        let id = *self.watcher.started().last().unwrap();
        self.cycle().release(id).unwrap();
    }

    /// From now on, notes for every pane listing whether the cycle lock was held.
    pub fn watch_cycle_lock(&self) {
        *self.herdr.lock_probe.borrow_mut() = Some(self._state_dir.path().join("cycle.lock"));
    }

    pub fn cycle_lock_was_held_when_herdr_was_asked(&self) -> Vec<bool> {
        self.herdr.lock_held_when_asked.borrow().clone()
    }

    pub fn focused_pane(&self) -> String {
        self.herdr.focused.borrow().clone().unwrap()
    }

    pub fn history_order(&self) -> Vec<String> {
        self.history.order()
    }
}
