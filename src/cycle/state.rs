use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::history::Snapshot;

// Only where the platform cannot tell when Alt comes up.
pub(super) const IDLE_MILLIS: u64 = 1500;
// A held cycle whose release watcher died still ends eventually.
pub const MAX_HOLD_MILLIS: u64 = 30_000;
// Hooks for the last stops may still be running just after Alt comes up.
pub(super) const RELEASE_GRACE_MILLIS: u64 = 500;

/// Names one cycle, so a release watcher only ever ends the cycle it was started for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleId(u64);

impl fmt::Display for CycleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for CycleId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(CycleId)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Older,
    Newer,
}

pub(super) struct Sighting {
    pub(super) live_mru: Vec<String>,
    pub(super) focused_pane: Option<String>,
    pub(super) baseline: Snapshot,
    pub(super) alt_held: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct State {
    // Index 0 is the pane the cycle started from.
    stops: Vec<String>,
    baseline: Snapshot,
    cursor: usize,
    visited: Vec<String>,
    last_press_at: u64,
    alt_held_at_start: bool,
    released_at: Option<u64>,
    started_at: u64,
}

impl State {
    pub(super) fn ongoing(previous: Option<State>, now: u64) -> Option<State> {
        previous.filter(|state| state.is_active(now))
    }

    pub(super) fn advance(
        ongoing: Option<State>,
        sighting: Sighting,
        direction: Direction,
        now: u64,
    ) -> Option<State> {
        let state = match ongoing {
            Some(state) => state.without_closed_panes(&sighting.live_mru),
            None => State::begin(sighting, now),
        };
        state.step(direction, now)
    }

    fn begin(sighting: Sighting, now: u64) -> State {
        let mut stops = sighting.live_mru;
        if let Some(focused) = sighting.focused_pane {
            stops.retain(|pane_id| *pane_id != focused);
            stops.insert(0, focused);
        }
        State {
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

    fn without_closed_panes(mut self, live: &[String]) -> State {
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

    fn step(mut self, direction: Direction, now: u64) -> Option<State> {
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

    pub(super) fn released(self, now: u64) -> State {
        State {
            released_at: Some(now),
            ..self
        }
    }

    pub(super) fn is_active(&self, now: u64) -> bool {
        let since_press = now.saturating_sub(self.last_press_at);
        if self.alt_held_at_start {
            self.released_at.is_none() && since_press <= MAX_HOLD_MILLIS
        } else {
            since_press <= IDLE_MILLIS
        }
    }

    // Focus events the cycle caused must stay out of history, or the panes it
    // only passed through would reorder.
    pub(super) fn has_visited(&self, pane_id: &str) -> bool {
        self.visited.iter().any(|visited| visited == pane_id)
    }

    pub(super) fn may_still_cause_focus_events(&self, now: u64) -> bool {
        self.is_active(now)
            || self
                .released_at
                .is_some_and(|released| now.saturating_sub(released) <= RELEASE_GRACE_MILLIS)
    }

    pub(super) fn needs_release_watcher(&self) -> bool {
        self.alt_held_at_start
    }

    pub(super) fn id(&self) -> CycleId {
        CycleId(self.started_at)
    }

    pub(super) fn baseline(&self) -> &Snapshot {
        &self.baseline
    }

    pub(super) fn target(&self) -> &str {
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
            baseline: Snapshot::default(),
            alt_held: false,
        }
    }

    fn see_with_alt_held(panes: &[&str]) -> Sighting {
        Sighting {
            alt_held: true,
            ..see(panes)
        }
    }

    fn begin(panes: &[&str], direction: Direction) -> State {
        State::advance(None, see(panes), direction, 5000).unwrap()
    }

    fn press(state: State, panes: &[&str], direction: Direction, now: u64) -> State {
        let ongoing = State::ongoing(Some(state), now);
        State::advance(ongoing, see(panes), direction, now).unwrap()
    }

    fn begin_holding_alt(panes: &[&str]) -> State {
        State::advance(None, see_with_alt_held(panes), Direction::Older, 5000).unwrap()
    }

    fn press_holding_alt(state: State, panes: &[&str], now: u64) -> State {
        let ongoing = State::ongoing(Some(state), now);
        State::advance(ongoing, see_with_alt_held(panes), Direction::Older, now).unwrap()
    }

    #[test]
    fn presses_within_the_idle_window_keep_stepping_older() {
        let first = begin(&["A", "B", "C"], Direction::Older);
        let second = press(first, &["B", "A", "C"], Direction::Older, 5100);

        assert_eq!(second.target(), "C");
    }

    #[test]
    fn stepping_wraps_around_in_both_directions() {
        let at_last = begin(&["A", "B", "C"], Direction::Newer);
        let wrapped = press(at_last.clone(), &["C", "A", "B"], Direction::Older, 5100);

        assert_eq!(at_last.target(), "C");
        assert_eq!(wrapped.target(), "A");
    }

    #[test]
    fn a_single_pane_stays_put() {
        let state = begin(&["A"], Direction::Older);

        assert_eq!(state.target(), "A");
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
        let state = State::advance(None, lagging, Direction::Older, 5000).unwrap();

        assert_eq!(state.target(), "A");
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
    fn a_held_cycle_whose_watcher_vanished_expires_eventually() {
        let first = begin_holding_alt(&["A", "B", "C"]);
        let much_later = press_holding_alt(first, &["B", "A", "C"], 5000 + MAX_HOLD_MILLIS + 1);

        assert_eq!(much_later.target(), "A");
    }
}
