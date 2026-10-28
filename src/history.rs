use std::cmp::Reverse;

use serde::{Deserialize, Serialize};

use crate::state_file::StateFile;

const MAX_VISITS: usize = 300;
// A visit further behind the newest than any hook can lag means the wall clock stepped back.
const MAX_HOOK_LAG_MILLIS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub pane_id: String,
    pub focused_at: u64,
}

pub struct History {
    file: StateFile,
}

impl History {
    pub fn new(file: StateFile) -> Self {
        Self { file }
    }

    pub fn visits(&self) -> Vec<Visit> {
        let mut visits: Vec<Visit> = self
            .file
            .read()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        visits.sort_by_key(|visit| Reverse(visit.focused_at));
        visits
    }

    // Hooks run concurrently and finish out of order, so `observed_at` decides
    // where the pane lands, not the order in which hooks get here.
    pub fn record(&self, pane_id: &str, observed_at: u64) -> anyhow::Result<()> {
        let _turn = self.file.lock()?;
        let mut visits = self.visits();
        let observed_at = unless_clock_stepped_back(observed_at, &visits);
        if has_later_visit(&visits, pane_id, observed_at) {
            return Ok(());
        }
        visits.retain(|visit| visit.pane_id != pane_id);
        insert_chronologically(
            &mut visits,
            Visit {
                pane_id: pane_id.to_string(),
                focused_at: observed_at,
            },
        );
        visits.truncate(MAX_VISITS);
        self.save(&visits)
    }

    /// Makes history read as if only `promoted` was focused since `baseline`
    /// was taken, however many panes a cycle passed through on the way.
    pub fn promote_after_cycle(
        &self,
        baseline: &[Visit],
        promoted: &str,
        promoted_at: u64,
    ) -> anyhow::Result<()> {
        let _turn = self.file.lock()?;
        let mut visits = self.visits();
        for visit in &mut visits {
            if let Some(before) = baseline.iter().find(|b| b.pane_id == visit.pane_id) {
                visit.focused_at = before.focused_at;
            }
        }
        visits.sort_by_key(|visit| Reverse(visit.focused_at));
        visits.retain(|visit| visit.pane_id != promoted);
        let newest = visits.first().map_or(0, |visit| visit.focused_at);
        visits.insert(
            0,
            Visit {
                pane_id: promoted.to_string(),
                focused_at: promoted_at.max(newest),
            },
        );
        self.save(&visits)
    }

    fn save(&self, visits: &[Visit]) -> anyhow::Result<()> {
        Ok(self.file.write(&serde_json::to_string(visits)?)?)
    }
}

fn unless_clock_stepped_back(observed_at: u64, visits: &[Visit]) -> u64 {
    let newest = visits.first().map_or(0, |visit| visit.focused_at);
    if observed_at.saturating_add(MAX_HOOK_LAG_MILLIS) < newest {
        newest
    } else {
        observed_at
    }
}

fn has_later_visit(visits: &[Visit], pane_id: &str, observed_at: u64) -> bool {
    visits
        .iter()
        .any(|visit| visit.pane_id == pane_id && visit.focused_at > observed_at)
}

fn insert_chronologically(visits: &mut Vec<Visit>, visit: Visit) {
    let position = visits
        .iter()
        .position(|existing| existing.focused_at <= visit.focused_at)
        .unwrap_or(visits.len());
    visits.insert(position, visit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn history_in(dir: &TempDir) -> History {
        History::new(StateFile::at(dir.path().join("history.json")))
    }

    fn pane_ids(history: &History) -> Vec<String> {
        history.visits().into_iter().map(|v| v.pane_id).collect()
    }

    #[test]
    fn nothing_recorded_means_no_visits() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);

        let visits = history.visits();

        assert!(visits.is_empty());
    }

    #[test]
    fn the_most_recently_focused_pane_comes_first() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        history.record("A", 1).unwrap();
        history.record("B", 2).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["B", "A"]);
    }

    #[test]
    fn focusing_a_pane_again_moves_it_to_the_front_without_duplicating_it() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        history.record("A", 1).unwrap();
        history.record("B", 2).unwrap();
        history.record("A", 3).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["A", "B"]);
    }

    #[test]
    fn history_never_holds_more_than_the_cap() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        for n in 0..=MAX_VISITS as u64 {
            history.record(&format!("pane-{n}"), n).unwrap();
        }

        let held = history.visits().len();

        assert_eq!(held, MAX_VISITS);
    }

    #[test]
    fn history_over_the_cap_forgets_the_oldest_pane() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        for n in 0..=MAX_VISITS as u64 {
            history.record(&format!("pane-{n}"), n).unwrap();
        }

        let order = pane_ids(&history);

        assert!(!order.contains(&"pane-0".to_string()));
    }

    #[test]
    fn a_late_hook_for_an_older_focus_lands_behind_newer_ones() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        history.record("A", 1).unwrap();
        history.record("C", 3).unwrap();
        history.record("B", 2).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["C", "B", "A"]);
    }

    #[test]
    fn a_late_hook_does_not_demote_a_pane_focused_since() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        history.record("A", 5).unwrap();
        history.record("A", 2).unwrap();

        let focused_at = history.visits()[0].focused_at;

        assert_eq!(focused_at, 5);
    }

    #[test]
    fn hooks_finishing_in_any_order_leave_history_in_focus_order() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        std::thread::scope(|scope| {
            for observed_at in [3u64, 7, 1, 5, 2, 8, 4, 6] {
                let history = &history;
                scope.spawn(move || {
                    history
                        .record(&format!("pane-{observed_at}"), observed_at)
                        .unwrap();
                });
            }
        });

        let focus_times: Vec<_> = history.visits().iter().map(|v| v.focused_at).collect();

        assert_eq!(focus_times, [8, 7, 6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn a_visit_recorded_into_an_unsorted_file_lands_in_chronological_place() {
        let dir = TempDir::new().unwrap();
        let file = StateFile::at(dir.path().join("history.json"));
        file.write(
            r#"[{"pane_id":"B","focused_at":1},{"pane_id":"C","focused_at":3},{"pane_id":"A","focused_at":0}]"#,
        )
        .unwrap();
        let history = History::new(file);
        history.record("D", 2).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["C", "D", "B", "A"]);
    }

    #[test]
    fn a_focus_far_behind_the_newest_is_a_clock_step_back_not_a_late_hook() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        let now = 10_000_000;
        history.record("A", now - 10).unwrap();
        history.record("B", now).unwrap();
        history.record("A", now - 1_000_000).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["A", "B"]);
    }

    #[test]
    fn corrupt_history_reads_as_empty() {
        let dir = TempDir::new().unwrap();
        let file = StateFile::at(dir.path().join("history.json"));
        file.write("{{{").unwrap();
        let history = History::new(file);

        let visits = history.visits();

        assert!(visits.is_empty());
    }

    fn history_of_three(dir: &TempDir) -> History {
        let history = history_in(dir);
        history.record("A", 10).unwrap();
        history.record("B", 20).unwrap();
        history.record("C", 30).unwrap();
        history
    }

    #[test]
    fn panes_a_cycle_passed_through_keep_their_original_order() {
        let dir = TempDir::new().unwrap();
        let history = history_of_three(&dir);
        let baseline = history.visits();
        history.promote_after_cycle(&baseline, "B", 100).unwrap();
        history.promote_after_cycle(&baseline, "A", 200).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["A", "C", "B"]);
    }

    #[test]
    fn promoting_after_the_clock_stepped_back_still_reaches_the_front() {
        let dir = TempDir::new().unwrap();
        let history = history_in(&dir);
        let now = 10_000_000;
        history.record("A", now).unwrap();
        history.record("B", now - 1).unwrap();
        let baseline = history.visits();
        history
            .promote_after_cycle(&baseline, "B", now - 5_000_000)
            .unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["B", "A"]);
    }

    #[test]
    fn promoting_a_pane_with_no_history_adds_it_at_the_front() {
        let dir = TempDir::new().unwrap();
        let history = history_of_three(&dir);
        let baseline = history.visits();
        history.promote_after_cycle(&baseline, "X", 100).unwrap();

        let order = pane_ids(&history);

        assert_eq!(order, ["X", "C", "B", "A"]);
    }
}
