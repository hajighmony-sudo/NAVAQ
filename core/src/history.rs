//! Bounded play history and play counts. The host records a play when a track starts.

use crate::model::TrackId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayHistory {
    capacity: usize,
    /// Oldest first.
    entries: Vec<TrackId>,
    counts: HashMap<TrackId, u32>,
}

impl Default for PlayHistory {
    fn default() -> Self {
        Self::new(200)
    }
}

impl PlayHistory {
    /// `capacity` is the number of recent entries kept (minimum 1). Counts are unbounded.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Vec::new(),
            counts: HashMap::new(),
        }
    }

    pub fn record_play(&mut self, id: TrackId) {
        *self.counts.entry(id.clone()).or_insert(0) += 1;
        self.entries.push(id);
        if self.entries.len() > self.capacity {
            let excess = self.entries.len() - self.capacity;
            self.entries.drain(..excess);
        }
    }

    /// Most recent plays first (a repeated track appears each time it was played).
    pub fn recent(&self, limit: usize) -> Vec<&TrackId> {
        self.entries.iter().rev().take(limit).collect()
    }

    pub fn play_count(&self, id: &TrackId) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// Tracks by descending play count (ties broken by id for determinism).
    pub fn most_played(&self, limit: usize) -> Vec<(&TrackId, u32)> {
        let mut all: Vec<_> = self.counts.iter().map(|(id, &n)| (id, n)).collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0 .0.cmp(&b.0 .0)));
        all.truncate(limit);
        all
    }

    /// Forget a track entirely (for example when it leaves the library).
    pub fn forget(&mut self, id: &TrackId) {
        self.entries.retain(|t| t != id);
        self.counts.remove(id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.counts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_recent_entries_but_all_counts() {
        let mut history = PlayHistory::new(2);
        for id in ["a", "b", "a", "c"] {
            history.record_play(id.into());
        }
        let recent: Vec<_> = history
            .recent(10)
            .into_iter()
            .map(|t| t.0.as_str())
            .collect();
        assert_eq!(recent, ["c", "a"]);
        assert_eq!(history.play_count(&"a".into()), 2);
        assert_eq!(history.play_count(&"zzz".into()), 0);
    }

    #[test]
    fn most_played_is_sorted_and_forget_removes_everything() {
        let mut history = PlayHistory::default();
        for id in ["b", "a", "a", "c", "b", "a"] {
            history.record_play(id.into());
        }
        let top: Vec<_> = history
            .most_played(2)
            .into_iter()
            .map(|(t, n)| (t.0.clone(), n))
            .collect();
        assert_eq!(top, [("a".to_string(), 3), ("b".to_string(), 2)]);
        history.forget(&"a".into());
        assert_eq!(history.play_count(&"a".into()), 0);
        assert!(history.recent(10).iter().all(|t| t.0 != "a"));
        history.clear();
        assert!(history.most_played(5).is_empty());
    }
}
