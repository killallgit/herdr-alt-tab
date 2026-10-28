use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use tempfile::TempDir;

use crate::clock::Clock;
use crate::connection::Connection;
use crate::cycler::Cycler;
use crate::herdr::{Herdr, HerdrError, Pane, Panes};
use crate::history::History;
use crate::keyboard::Keyboard;
use crate::release::ReleaseWatcher;
use crate::session::{Direction, Session, Sighting};
use crate::session_store::SessionStore;
use crate::track::Tracker;

pub fn session_holding_alt(started_at: u64) -> Session {
    let sighting = Sighting {
        live_mru: vec!["A".into(), "B".into()],
        focused_pane: Some("A".into()),
        baseline: Vec::new(),
        alt_held: true,
    };
    Session::advance(None, sighting, Direction::Older, started_at).unwrap()
}

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
}

pub struct FakeHerdr {
    panes: RefCell<Vec<String>>,
    focused: RefCell<Option<String>>,
    focus_requests: RefCell<Vec<String>>,
    focus_fails: Cell<bool>,
}

impl FakeHerdr {
    fn with_panes(ids: &[&str]) -> Self {
        Self {
            panes: RefCell::new(ids.iter().map(|id| id.to_string()).collect()),
            focused: RefCell::new(ids.first().map(|id| id.to_string())),
            focus_requests: RefCell::new(Vec::new()),
            focus_fails: Cell::new(false),
        }
    }

    pub fn open(&self, pane_id: &str) {
        self.panes.borrow_mut().push(pane_id.to_string());
    }

    pub fn close(&self, pane_id: &str) {
        self.panes.borrow_mut().retain(|id| id != pane_id);
    }

    pub fn fail_focus(&self) {
        self.focus_fails.set(true);
    }

    pub fn focus_requests(&self) -> Vec<String> {
        self.focus_requests.borrow().clone()
    }
}

impl Herdr for FakeHerdr {
    fn panes(&self) -> Result<Panes, HerdrError> {
        let focused = self.focused.borrow();
        Ok(Panes {
            panes: self
                .panes
                .borrow()
                .iter()
                .map(|id| Pane {
                    id: id.clone(),
                    focused: focused.as_deref() == Some(id),
                })
                .collect(),
        })
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

pub struct FakeWatcher(RefCell<Vec<u64>>);

impl FakeWatcher {
    pub fn started(&self) -> Vec<u64> {
        self.0.borrow().clone()
    }
}

impl ReleaseWatcher for FakeWatcher {
    fn watch(&self, started_at: u64) -> anyhow::Result<()> {
        self.0.borrow_mut().push(started_at);
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
    pub sessions: SessionStore,
    pub clock: FakeClock,
    _state_dir: TempDir,
}

impl Rig {
    pub fn with_panes(panes: &[&str]) -> Self {
        let state_dir = TempDir::new().unwrap();
        let connection = Connection::new(state_dir.path().to_path_buf(), None);
        let history = History::new(connection.state_file("history"));
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
            sessions: SessionStore::new(connection.state_file("session")),
            clock: FakeClock::at(10_000),
            _state_dir: state_dir,
        }
    }

    pub fn press(&self, direction: Direction) -> anyhow::Result<()> {
        Cycler {
            herdr: &self.herdr,
            keyboard: &self.keyboard,
            watcher: &self.watcher,
            history: &self.history,
            sessions: &self.sessions,
            clock: &self.clock,
        }
        .press(direction)
    }

    pub fn tracker(&self) -> Tracker<'_> {
        Tracker {
            herdr: &self.herdr,
            history: &self.history,
            sessions: &self.sessions,
            clock: &self.clock,
        }
    }

    pub fn pass_time(&self, millis: u64) {
        self.clock.0.set(self.clock.0.get() + millis);
    }

    pub fn user_focuses(&self, pane_id: &str) {
        *self.herdr.focused.borrow_mut() = Some(pane_id.to_string());
    }

    pub fn release_alt(&self) {
        let started_at = self.sessions.load().unwrap().started_at();
        self.sessions
            .mark_released(started_at, self.clock.now_millis())
            .unwrap();
    }

    pub fn focused_pane(&self) -> String {
        self.herdr.focused.borrow().clone().unwrap()
    }

    pub fn history_order(&self) -> Vec<String> {
        self.history
            .visits()
            .into_iter()
            .map(|visit| visit.pane_id)
            .collect()
    }
}
