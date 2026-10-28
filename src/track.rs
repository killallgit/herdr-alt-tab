use serde::Deserialize;

use crate::clock::Clock;
use crate::herdr::Herdr;
use crate::history::History;
use crate::session::Session;
use crate::session_store::SessionStore;

const PANE_FOCUSED_EVENT: &str = "pane_focused";

#[derive(Deserialize)]
struct HookEvent {
    event: String,
    #[serde(default)]
    data: HookEventData,
}

#[derive(Deserialize, Default)]
struct HookEventData {
    pane_id: Option<String>,
}

pub fn announced_pane(event_json: &str) -> Result<Option<String>, serde_json::Error> {
    let event: HookEvent = serde_json::from_str(event_json)?;
    Ok(event
        .data
        .pane_id
        .filter(|_| event.event == PANE_FOCUSED_EVENT))
}

#[derive(PartialEq, Eq)]
enum FocusSource {
    Cycle,
    User,
}

pub struct Tracker<'a> {
    pub herdr: &'a dyn Herdr,
    pub history: &'a History,
    pub sessions: &'a SessionStore,
    pub clock: &'a dyn Clock,
}

impl Tracker<'_> {
    // Herdr does not fire `pane.focused` for every switch, so whatever the
    // event was, the pane focused right now is recorded too.
    pub fn track(&self, announced_pane: Option<&str>) -> anyhow::Result<()> {
        let observed_at = self.clock.now_millis();
        if self.end_cycle_if_user_moved(observed_at)? == FocusSource::Cycle {
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

    fn end_cycle_if_user_moved(&self, now: u64) -> anyhow::Result<FocusSource> {
        let _turn = self.sessions.lock()?;
        let Some(cycle) = self
            .sessions
            .load()
            .filter(|cycle| cycle.may_still_cause_focus_events(now))
        else {
            return Ok(FocusSource::User);
        };
        if self.cycle_moved_focus(&cycle) {
            return Ok(FocusSource::Cycle);
        }
        self.sessions.clear();
        Ok(FocusSource::User)
    }

    // Focus that cannot be read is assumed to be the cycle's.
    fn cycle_moved_focus(&self, cycle: &Session) -> bool {
        match self.herdr.panes() {
            Ok(panes) => panes
                .focused()
                .is_none_or(|pane| cycle.has_visited(&pane.id)),
            Err(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Direction::Older;
    use crate::test_support::Rig;

    #[test]
    fn a_pane_focused_event_names_its_pane() {
        let event = r#"{"event":"pane_focused","data":{"pane_id":"p1","workspace_id":"w1"}}"#;

        let pane = announced_pane(event).unwrap();

        assert_eq!(pane.as_deref(), Some("p1"));
    }

    #[test]
    fn other_events_name_no_pane() {
        let event = r#"{"event":"workspace_focused","data":{"workspace_id":"w1"}}"#;

        let pane = announced_pane(event).unwrap();

        assert_eq!(pane, None);
    }

    #[test]
    fn a_malformed_event_is_a_json_syntax_error() {
        let error = announced_pane("not json").unwrap_err();

        assert!(error.is_syntax());
    }

    #[test]
    fn moving_by_hand_puts_the_focused_pane_first_in_history() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.user_focuses("C");
        rig.tracker().track(Some("C")).unwrap();

        let order = rig.history_order();

        assert_eq!(order, ["C", "A", "B"]);
    }

    #[test]
    fn the_focused_pane_is_recorded_even_when_the_event_names_none() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.user_focuses("B");
        rig.tracker().track(None).unwrap();

        let order = rig.history_order();

        assert_eq!(order, ["B", "A", "C"]);
    }

    #[test]
    fn focus_a_cycle_caused_is_left_out_of_history() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        let history_after_cycle = rig.history.visits();
        rig.pass_time(10);
        rig.tracker().track(Some("B")).unwrap();

        let history_after_hook = rig.history.visits();

        assert_eq!(history_after_hook, history_after_cycle);
    }

    #[test]
    fn moving_by_hand_during_a_cycle_ends_it() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.user_focuses("C");
        rig.tracker().track(Some("C")).unwrap();

        let session = rig.sessions.load();

        assert!(session.is_none());
    }

    #[test]
    fn moving_by_hand_during_a_cycle_is_recorded() {
        let rig = Rig::with_panes(&["A", "B", "C"]);
        rig.press(Older).unwrap();
        rig.user_focuses("C");
        rig.tracker().track(Some("C")).unwrap();

        let order = rig.history_order();

        assert_eq!(order[0], "C");
    }
}
