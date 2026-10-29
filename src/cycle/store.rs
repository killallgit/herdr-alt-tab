use crate::lock::{Held, Lock};
use crate::state_file::StateFile;

use super::state::State;

pub(super) struct Store {
    file: StateFile,
    lock: Lock,
}

impl Store {
    pub(super) fn new(file: StateFile, lock: Lock) -> Self {
        Self { file, lock }
    }

    /// The only way to read or change the saved state, so nobody does it unlocked.
    pub(super) fn begin_turn(&self) -> std::io::Result<Turn<'_>> {
        Ok(Turn {
            store: self,
            _held: self.lock.acquire()?,
        })
    }
}

pub(super) struct Turn<'a> {
    store: &'a Store,
    _held: Held,
}

impl Turn<'_> {
    pub(super) fn load(&self) -> Option<State> {
        serde_json::from_str(&self.store.file.read()?).ok()
    }

    pub(super) fn save(&self, state: &State) -> anyhow::Result<()> {
        Ok(self.store.file.write(&serde_json::to_string(state)?)?)
    }

    pub(super) fn clear(&self) {
        self.store.file.remove();
    }
}
