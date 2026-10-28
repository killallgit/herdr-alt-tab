use serde::{Deserialize, Serialize};

use crate::history::Visit;

// Only where the platform cannot tell when Alt comes up.
const IDLE_MILLIS: u64 = 1500;
// A held cycle whose release watcher died still ends eventually.
pub(crate) const MAX_HOLD_MILLIS: u64 = 30_000;
// Hooks for the last stops may still be running just after Alt comes up.
const RELEASE_GRACE_MILLIS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Older,
    Newer,
}

pub struct Sighting {
    pub live_mru: Vec<String>,
    pub focused_pane: Option<String>,
    pub baseline: Vec<Visit>,
    pub alt_held: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    // Index 0 is the pane the cycle started from.
    stops: Vec<String>,
    baseline: Vec<Visit>,
    cursor: usize,
    visited: Vec<String>,
    last_press_at: u64,
    alt_held_at_start: bool,
    released_at: Option<u64>,
    started_at: u64,
}

impl Session {
    pub fn ongoing(previous: Option<Session>, now: u64) -> Option<Session> {
        previous.filter(|session| session.is_active(now))
    }

    pub fn advance(
        ongoing: Option<Session>,
        sighting: Sighting,
        direction: Direction,
        now: u64,
    ) -> Option<Session> {
        let session = match ongoing {
            Some(session) => session.without_closed_panes(&sighting.live_mru),
            None => Session::begin(sighting, now),
        };
        session.step(direction, now)
    }

    fn begin(sighting: Sighting, now: u64) -> Session {
        let mut stops = sighting.live_mru;
        if let Some(focused) = sighting.focused_pane {
            stops.retain(|pane_id| *pane_id != focused);
            stops.insert(0, focused);
        }
        Session {
            stops,
            baseline: sighting.baseline,
            cursor: 0,
            visited: Vec::new(),
            last_press_at: now,
            alt_held_at_start: sighting.alt_held,
            released_at: None,
            started_at: now,
        }
    }

    fn without_closed_panes(mut self, live: &[String]) -> Session {
        let is_live = |pane_id: &String| live.contains(pane_id);
        let current = self.stops.get(self.cursor).cloned();
        let live_before_cursor = self.stops[..self.cursor.min(self.stops.len())]
            .iter()
            .filter(|pane_id| is_live(pane_id))
            .count();
        self.stops.retain(is_live);
        self.cursor = match current {
            Some(current) if self.stops.contains(&current) => live_before_cursor,
            _ => live_before_cursor.saturating_sub(1),
        };
        self
    }

    fn step(mut self, direction: Direction, now: u64) -> Option<Session> {
        let count = self.stops.len();
        if count == 0 {
            return None;
        }
        self.cursor = match direction {
            Direction::Older => (self.cursor + 1) % count,
            Direction::Newer => (self.cursor + count - 1) % count,
        };
        self.visited.push(self.stops[self.cursor].clone());
        self.last_press_at = now;
        Some(self)
    }

    pub fn released(self, now: u64) -> Session {
        Session {
            released_at: Some(now),
            ..self
        }
    }

    pub fn is_active(&self, now: u64) -> bool {
        let since_press = now.saturating_sub(self.last_press_at);
        if self.alt_held_at_start {
            self.released_at.is_none() && since_press <= MAX_HOLD_MILLIS
        } else {
            since_press <= IDLE_MILLIS
        }
    }

    // Focus events the cycle caused must stay out of history, or the panes it
    // only passed through would reorder.
    pub fn has_visited(&self, pane_id: &str) -> bool {
        self.visited.iter().any(|visited| visited == pane_id)
    }

    pub fn may_still_cause_focus_events(&self, now: u64) -> bool {
        self.is_active(now)
            || self
                .released_at
                .is_some_and(|released| now.saturating_sub(released) <= RELEASE_GRACE_MILLIS)
    }

    pub fn needs_release_watcher(&self) -> bool {
        self.alt_held_at_start
    }

    pub fn started_at(&self) -> u64 {
        self.started_at
    }

    pub fn baseline(&self) -> &[Visit] {
        &self.baseline
    }

    pub fn target(&self) -> &str {
        &self.stops[self.cursor]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(panes: &[&str]) -> Vec<String> {
        panes.iter().map(|id| id.to_string()).collect()
    }

    // Focused on the most recently used pane, as when history is up to date.
    fn see(panes: &[&str]) -> Sighting {
        Sighting {
            live_mru: ids(panes),
            focused_pane: panes.first().map(|id| id.to_string()),
            baseline: Vec::new(),
            alt_held: false,
        }
    }

    fn see_with_alt_held(panes: &[&str]) -> Sighting {
        Sighting {
            alt_held: true,
            ..see(panes)
        }
    }

    fn begin(panes: &[&str], direction: Direction) -> Session {
        Session::advance(None, see(panes), direction, 5000).unwrap()
    }

    fn press(session: Session, panes: &[&str], direction: Direction, now: u64) -> Session {
        let ongoing = Session::ongoing(Some(session), now);
        Session::advance(ongoing, see(panes), direction, now).unwrap()
    }

    fn begin_holding_alt(panes: &[&str]) -> Session {
        Session::advance(None, see_with_alt_held(panes), Direction::Older, 5000).unwrap()
    }

    fn press_holding_alt(session: Session, panes: &[&str], now: u64) -> Session {
        let ongoing = Session::ongoing(Some(session), now);
        Session::advance(ongoing, see_with_alt_held(panes), Direction::Older, now).unwrap()
    }

    #[test]
    fn first_older_press_targets_the_second_most_recent_pane() {
        let session = begin(&["A", "B", "C"], Direction::Older);

        assert_eq!(session.target(), "B");
    }

    #[test]
    fn presses_within_the_idle_window_keep_stepping_older() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["B", "A", "C"], Direction::Older, 5100);

        assert_eq!(second.target(), "C");
    }

    #[test]
    fn newer_press_steps_back_toward_the_origin() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["B", "A", "C"], Direction::Older, 5100);
        let third = press(second, &["C", "A", "B"], Direction::Newer, 5200);

        assert_eq!(third.target(), "B");
    }

    #[test]
    fn newer_from_the_origin_wraps_to_the_last_pane() {
        let session = begin(&["A", "B", "C"], Direction::Newer);

        assert_eq!(session.target(), "C");
    }

    #[test]
    fn older_from_the_last_pane_wraps_to_the_origin() {
        let at_last = begin(&["A", "B", "C"], Direction::Newer);
        let wrapped = press(at_last, &["C", "A", "B"], Direction::Older, 5100);

        assert_eq!(wrapped.target(), "A");
    }

    #[test]
    fn a_single_pane_stays_put() {
        let session = begin(&["A"], Direction::Older);

        assert_eq!(session.target(), "A");
    }

    #[test]
    fn no_live_panes_means_nothing_to_cycle_to() {
        let session = Session::advance(None, see(&[]), Direction::Older, 5000);

        assert!(session.is_none());
    }

    #[test]
    fn a_press_after_the_idle_window_starts_a_fresh_cycle() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let later = press(
            first,
            &["B", "A", "C"],
            Direction::Older,
            5000 + IDLE_MILLIS + 1,
        );

        assert_eq!(later.target(), "A");
    }

    #[test]
    fn the_origin_is_the_focused_pane_even_when_history_lags() {
        let lagging = Sighting {
            focused_pane: Some("B".to_string()),
            ..see(&["A", "B", "C"])
        };
        let session = Session::advance(None, lagging, Direction::Older, 5000).unwrap();

        assert_eq!(session.target(), "A");
    }

    #[test]
    fn a_focused_pane_missing_from_history_is_still_the_origin() {
        let untracked = Sighting {
            focused_pane: Some("X".to_string()),
            ..see(&["A", "B"])
        };
        let session = Session::advance(None, untracked, Direction::Older, 5000).unwrap();

        assert_eq!(session.target(), "A");
    }

    #[test]
    fn a_pane_closed_mid_cycle_is_never_targeted() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["A", "B"], Direction::Older, 5100);

        assert_eq!(second.target(), "A");
    }

    #[test]
    fn closing_the_current_stop_continues_from_its_neighbour() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["A", "C"], Direction::Older, 5100);

        assert_eq!(second.target(), "C");
    }

    #[test]
    fn panes_the_cycle_moved_to_count_as_visited() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["B", "A", "C"], Direction::Older, 5100);

        assert!(second.has_visited("B"));
        assert!(second.has_visited("C"));
    }

    #[test]
    fn panes_the_cycle_never_moved_to_do_not_count_as_visited() {
        let first = begin(&["A", "B", "C"], Direction::Older);

        assert!(!first.has_visited("A"));
        assert!(!first.has_visited("D"));
    }

    #[test]
    fn an_active_cycle_may_still_cause_focus_events() {
        let session = begin(&["A", "B", "C"], Direction::Older);

        assert!(session.may_still_cause_focus_events(5000 + IDLE_MILLIS));
    }

    #[test]
    fn an_idle_cycle_no_longer_causes_focus_events() {
        let session = begin(&["A", "B", "C"], Direction::Older);

        assert!(!session.may_still_cause_focus_events(5000 + IDLE_MILLIS + 1));
    }

    #[test]
    fn tapping_releasing_and_tapping_again_starts_a_fresh_cycle() {
        let tapped = begin_holding_alt(&["A", "B", "C"]).released(5050);
        let again = press_holding_alt(tapped, &["B", "A", "C"], 5100);

        assert_eq!(again.target(), "A");
    }

    #[test]
    fn holding_alt_keeps_one_cycle_across_a_long_pause() {
        let first = begin_holding_alt(&["A", "B", "C"]);
        let after_pause = press_holding_alt(first, &["B", "A", "C"], 5000 + 10 * IDLE_MILLIS);

        assert_eq!(after_pause.target(), "C");
    }

    #[test]
    fn a_held_cycle_whose_watcher_vanished_expires_eventually() {
        let first = begin_holding_alt(&["A", "B", "C"]);
        let much_later = press_holding_alt(first, &["B", "A", "C"], 5000 + MAX_HOLD_MILLIS + 1);

        assert_eq!(much_later.target(), "A");
    }

    #[test]
    fn a_released_cycle_may_still_cause_focus_events_for_a_moment() {
        let released = begin_holding_alt(&["A", "B", "C"]).released(5100);

        assert!(released.may_still_cause_focus_events(5100 + RELEASE_GRACE_MILLIS));
    }

    #[test]
    fn a_released_cycle_stops_causing_focus_events_after_the_grace_period() {
        let released = begin_holding_alt(&["A", "B", "C"]).released(5100);

        assert!(!released.may_still_cause_focus_events(5100 + RELEASE_GRACE_MILLIS + 1));
    }
}
