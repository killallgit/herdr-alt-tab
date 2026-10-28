use crate::clock::Clock;
use crate::herdr::{Herdr, Panes};
use crate::history::{History, Visit};
use crate::keyboard::Keyboard;
use crate::release::ReleaseWatcher;
use crate::session::{Direction, Session, Sighting};
use crate::session_store::SessionStore;

pub struct Cycler<'a> {
    pub herdr: &'a dyn Herdr,
    pub keyboard: &'a dyn Keyboard,
    pub watcher: &'a dyn ReleaseWatcher,
    pub history: &'a History,
    pub sessions: &'a SessionStore,
    pub clock: &'a dyn Clock,
}

impl Cycler<'_> {
    pub fn press(&self, direction: Direction) -> anyhow::Result<()> {
        let _turn = self.sessions.lock()?;
        let now = self.clock.now_millis();

        let panes = self.herdr.panes()?;
        let visits = self.history.visits();
        let sighting = Sighting {
            live_mru: live_panes_by_recency(&panes, &visits),
            focused_pane: panes.focused().map(|pane| pane.id.clone()),
            baseline: visits,
            alt_held: self.keyboard.alt_held() == Some(true),
        };

        let ongoing = Session::ongoing(self.sessions.load(), now);
        let starts_new_cycle = ongoing.is_none();
        let Some(session) = Session::advance(ongoing, sighting, direction, now) else {
            return Ok(());
        };

        // Saved before focusing so the track hook, which takes the same lock,
        // can tell the focus events that follow are ours.
        self.sessions.save(&session)?;
        if let Err(error) = self.herdr.focus(session.target()) {
            self.sessions.clear();
            return Err(error.into());
        }
        self.history
            .promote_after_cycle(session.baseline(), session.target(), now)?;
        if starts_new_cycle && session.needs_release_watcher() {
            self.watcher.watch(session.started_at())?;
        }
        Ok(())
    }
}

fn live_panes_by_recency(panes: &Panes, visits: &[Visit]) -> Vec<String> {
    let mut by_recency: Vec<String> = visits
        .iter()
        .map(|visit| visit.pane_id.clone())
        .filter(|pane_id| panes.contains(pane_id))
        .collect();
    let never_focused: Vec<String> = panes
        .ids()
        .filter(|pane_id| !by_recency.iter().any(|known| known == pane_id))
        .map(String::from)
        .collect();
    by_recency.extend(never_focused);
    by_recency
}

#[cfg(test)]
mod tests {
    use crate::herdr::HerdrError;
    use crate::session::Direction::{Newer, Older};
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
    fn panes_closed_since_they_were_last_focused_are_skipped() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.herdr.close("B");
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "C");
    }

    #[test]
    fn a_pane_never_focused_since_install_comes_after_every_known_pane() {
        let rig = Rig::with_panes(&["A", "B"]);
        rig.herdr.open("N");
        rig.press(Older).unwrap();
        rig.press(Older).unwrap();

        assert_eq!(rig.focused_pane(), "N");
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

        let session = rig.sessions.load();

        assert!(session.is_none());
    }

    #[test]
    fn with_no_panes_there_is_nothing_to_focus() {
        let rig = Rig::with_panes(&[]);
        rig.press(Older).unwrap();

        assert!(rig.herdr.focus_requests().is_empty());
    }
}
