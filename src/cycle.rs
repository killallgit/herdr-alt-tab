mod state;
mod store;

use crate::clock::Clock;
use crate::connection::Connection;
use crate::herdr::Herdr;
use crate::history::History;
use crate::keyboard::Keyboard;
use crate::release::ReleaseWatcher;
use state::{Sighting, State};
use store::{Store, Turn};

pub use state::{CycleId, Direction, MAX_HOLD_MILLIS};

pub struct Collaborators<'a> {
    pub herdr: &'a dyn Herdr,
    pub keyboard: &'a dyn Keyboard,
    pub watcher: &'a dyn ReleaseWatcher,
    pub history: &'a History,
    pub clock: &'a dyn Clock,
}

/// Runs the cycles of Alt+Tab presses, and tells them apart from the user's own moves.
///
/// Presses, focus hooks and the release watcher are separate processes, so each
/// takes a turn on the saved state, and holds it until its history writes are
/// done. The turn is always taken before History's own lock, never after.
pub struct Cycle<'a> {
    herdr: &'a dyn Herdr,
    keyboard: &'a dyn Keyboard,
    watcher: &'a dyn ReleaseWatcher,
    history: &'a History,
    clock: &'a dyn Clock,
    store: Store,
}

impl<'a> Cycle<'a> {
    pub fn new(connection: &Connection, with: Collaborators<'a>) -> Self {
        Self {
            herdr: with.herdr,
            keyboard: with.keyboard,
            watcher: with.watcher,
            history: with.history,
            clock: with.clock,
            store: Store::new(connection.state_file("cycle"), connection.lock("cycle")),
        }
    }

    pub fn press(&self, direction: Direction) -> anyhow::Result<()> {
        let turn = self.store.begin_turn()?;
        let now = self.clock.now_millis();

        let panes = self.herdr.panes()?;
        let recency = self.history.recency_of(panes.ids());
        let sighting = Sighting {
            live_mru: recency.live,
            focused_pane: panes.focused().map(|pane| pane.id.clone()),
            baseline: recency.snapshot,
            alt_held: self.keyboard.alt_held() == Some(true),
        };

        let ongoing = State::ongoing(turn.load(), now);
        let starts_new_cycle = ongoing.is_none();
        let Some(state) = State::advance(ongoing, sighting, direction, now) else {
            return Ok(());
        };

        // Saved before focusing so the focus hook, which takes the same turn,
        // can tell the focus events that follow are ours.
        turn.save(&state)?;
        if let Err(error) = self.herdr.focus(state.target()) {
            turn.clear();
            return Err(error.into());
        }
        self.history
            .promote_after_cycle(state.baseline(), state.target(), now)?;
        if starts_new_cycle && state.needs_release_watcher() {
            self.watcher.watch(state.id())?;
        }
        Ok(())
    }

    // Herdr does not fire `pane.focused` for every switch, so whatever the
    // event was, the pane focused right now is recorded too.
    pub fn observe_focus(&self, announced_pane: Option<&str>) -> anyhow::Result<()> {
        let observed_at = self.clock.now_millis();
        let turn = self.store.begin_turn()?;
        if self.cycle_caused_focus(&turn, observed_at) {
            return Ok(());
        }
        if let Some(pane_id) = announced_pane {
            self.history.record(pane_id, observed_at)?;
        }
        if let Some(focused) = self.herdr.panes()?.focused() {
            self.history.record(&focused.id, self.clock.now_millis())?;
        }
        Ok(())
    }

    /// Alt came up. Only ends the cycle the watcher was started for.
    pub fn release(&self, id: CycleId) -> anyhow::Result<()> {
        let turn = self.store.begin_turn()?;
        match turn.load() {
            Some(state) if state.id() == id => turn.save(&state.released(self.clock.now_millis())),
            _ => Ok(()),
        }
    }

    // A focus the user caused ends the cycle.
    fn cycle_caused_focus(&self, turn: &Turn, now: u64) -> bool {
        let Some(state) = turn
            .load()
            .filter(|state| state.may_still_cause_focus_events(now))
        else {
            return false;
        };
        if self.moved_focus(&state) {
            return true;
        }
        turn.clear();
        false
    }

    // Focus that cannot be read is assumed to be the cycle's. Its events must
    // stay out of history, or the panes it only passed through would reorder.
    fn moved_focus(&self, state: &State) -> bool {
        match self.herdr.panes() {
            Ok(panes) => panes
                .focused()
                .is_none_or(|pane| state.has_visited(&pane.id)),
            Err(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::state::{IDLE_MILLIS, RELEASE_GRACE_MILLIS};
    use crate::cycle::Direction::{Newer, Older};
    use crate::herdr::HerdrError;
    use crate::test_support::Rig;

    #[test]
    fn the_first_press_focuses_the_second_most_recent_pane() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "B");
    }

    #[test]
    fn holding_alt_keeps_walking_back_however_long_the_pauses() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.pass_time(10_000);
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "C");
    }

    #[test]
    fn newer_walks_forward_again() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();
        rig.press(Newer).unwrap();

        assert_eq!(rig.focused_pane(), "B");
    }

    #[test]
    fn tapping_letting_go_and_tapping_again_toggles_between_two_panes() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.release_alt();
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "A");
    }

    #[test]
    fn panes_passed_through_keep_their_place_in_history() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();

        let order = rig.history_order();

        assert_eq!(order, ["C", "A", "B"]);
    }

    #[test]
    fn a_cycle_started_from_an_untracked_pane_can_return_to_it() {
        let rig = Rig::with_panes(&["A", "B"]);
        rig.herdr.open("N");
        rig.user_focuses("N");
        rig.press(Older).unwrap();
        rig.press(Newer).unwrap();

        assert_eq!(rig.focused_pane(), "N");
    }

    #[test]
    fn a_cycle_begun_with_alt_held_gets_exactly_one_release_watcher() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();

        assert_eq!(rig.watcher.started().len(), 1);
    }

    #[test]
    fn a_cycle_begun_without_alt_held_gets_no_release_watcher() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.keyboard.set_alt(None);
        rig.press(Older).unwrap();

        assert!(rig.watcher.started().is_empty());
    }

    #[test]
    fn a_release_for_an_earlier_cycle_leaves_the_current_one_running() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.cycle().release("4000".parse().unwrap()).unwrap();
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "C");
    }

    #[test]
    fn a_failed_focus_is_reported_as_the_herdr_error() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.herdr.fail_focus();

        let error = rig.press(Older).unwrap_err();

        assert!(matches!(
            error.downcast_ref::<HerdrError>(),
            Some(HerdrError::Rejected { .. })
        ));
    }

    #[test]
    fn a_failed_focus_ends_the_cycle() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.herdr.fail_focus();
        let _ = rig.press(Older);
        rig.herdr.allow_focus();
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "B");
    }

    #[test]
    fn with_no_panes_there_is_nothing_to_focus() {
        let rig = Rig::with_panes(&[]);
        rig.press(Older).unwrap();

        assert!(rig.herdr.focus_requests().is_empty());
    }

    #[test]
    fn moving_by_hand_puts_the_focused_pane_first_in_history() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.user_focuses("C");
        rig.cycle().observe_focus(Some("C")).unwrap();

        let order = rig.history_order();

        assert_eq!(order, ["C", "A", "B"]);
    }

    #[test]
    fn the_focused_pane_is_recorded_even_when_the_event_names_none() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.user_focuses("B");
        rig.cycle().observe_focus(None).unwrap();

        let order = rig.history_order();

        assert_eq!(order, ["B", "A", "C"]);
    }

    #[test]
    fn focus_a_cycle_caused_is_left_out_of_history() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();
        let history_after_cycle = rig.history_order();
        rig.pass_time(10);
        rig.cycle().observe_focus(Some("B")).unwrap();

        let history_after_hook = rig.history_order();

        assert_eq!(history_after_hook, history_after_cycle);
    }

    #[test]
    fn moving_by_hand_during_a_cycle_ends_it_and_is_recorded() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.user_focuses("C");
        rig.cycle().observe_focus(Some("C")).unwrap();
        let recorded_first = rig.history_order()[0].clone();
        rig.press(Older).unwrap();

        assert_eq!(recorded_first, "C");
        assert_eq!(rig.focused_pane(), "B");
    }

    #[test]
    fn a_released_cycle_claims_focus_events_only_for_a_grace_period() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();
        rig.release_alt();
        let history_after_cycle = rig.history_order();

        rig.pass_time(RELEASE_GRACE_MILLIS);
        rig.cycle().observe_focus(Some("B")).unwrap();
        let within_grace = rig.history_order();
        rig.pass_time(1);
        rig.cycle().observe_focus(Some("B")).unwrap();
        let after_grace = rig.history_order();

        assert_eq!(within_grace, history_after_cycle);
        assert_ne!(after_grace, history_after_cycle);
    }

    #[test]
    fn without_alt_a_cycle_claims_focus_events_only_until_it_goes_idle() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.keyboard.set_alt(None);
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();
        let history_after_cycle = rig.history_order();

        rig.pass_time(IDLE_MILLIS);
        rig.cycle().observe_focus(Some("B")).unwrap();
        let while_active = rig.history_order();
        rig.pass_time(1);
        rig.cycle().observe_focus(Some("B")).unwrap();
        let once_idle = rig.history_order();

        assert_eq!(while_active, history_after_cycle);
        assert_ne!(once_idle, history_after_cycle);
    }
}
