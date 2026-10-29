use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use crate::clock::Clock;
use crate::command::Command as Invocation;
use crate::cycle::{Cycle, CycleId, MAX_HOLD_MILLIS};
use crate::keyboard::Keyboard;

const POLL_MILLIS: u64 = 15;

pub trait ReleaseWatcher {
    fn watch(&self, cycle: CycleId) -> anyhow::Result<()>;
}

// Outlives the action that started it, which exits as soon as it has focused a pane.
pub struct DetachedWatcher;

impl ReleaseWatcher for DetachedWatcher {
    fn watch(&self, cycle: CycleId) -> anyhow::Result<()> {
        Command::new(std::env::current_exe()?)
            .args(Invocation::Watch { cycle }.args())
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
    pub cycle: &'a Cycle<'a>,
    pub clock: &'a dyn Clock,
}

impl ReleaseWaiter<'_> {
    pub fn wait(&self, cycle: CycleId) -> anyhow::Result<()> {
        let give_up_at = self.clock.now_millis() + MAX_HOLD_MILLIS;
        while self.clock.now_millis() < give_up_at {
            if self.keyboard.alt_held() != Some(true) {
                return self.cycle.release(cycle);
            }
            self.clock.pause(POLL_MILLIS);
        }
        // Alt is still down, so the cycle is not over; it expires on its own
        // MAX_HOLD_MILLIS after its last press.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::Direction::Older;
    use crate::test_support::{FakeKeyboard, Rig};

    fn wait_for(rig: &Rig, keyboard: &FakeKeyboard) {
        let cycle = rig.cycle();
        ReleaseWaiter {
            keyboard,
            cycle: &cycle,
            clock: &rig.clock,
        }
        .wait(rig.watcher.started()[0])
        .unwrap();
    }

    #[test]
    fn the_cycle_ends_once_alt_comes_up() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        wait_for(
            &rig,
            &FakeKeyboard::reading(&[Some(true), Some(true), None]),
        );
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "A");
    }

    #[test]
    fn a_hold_that_never_ends_is_given_up_on_at_the_cap_without_ending_the_cycle() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        let keyboard = FakeKeyboard::reading(&[]);
        keyboard.set_alt(Some(true));
        let started = rig.clock.now_millis();
        wait_for(&rig, &keyboard);

        let waited = rig.clock.now_millis() - started;
        rig.press(Older).unwrap();

        assert_eq!(waited, MAX_HOLD_MILLIS);
        assert_eq!(rig.focused_pane(), "C");
    }
}
