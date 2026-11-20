//! Per-track resume positions (podcasts, audiobooks, long mixes).

use crate::model::TrackId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Positions earlier than this are not worth resuming.
pub const MIN_BOOKMARK_MS: u64 = 5_000;
/// Positions this close to the end count as finished.
pub const END_MARGIN_MS: u64 = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BookmarkStore {
    positions: HashMap<TrackId, u64>,
}

impl BookmarkStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember where playback stopped. Positions at the very start or near the end clear
    /// the bookmark instead. Returns true when a bookmark is stored.
    pub fn save(&mut self, id: &TrackId, position_ms: u64, duration_ms: Option<u64>) -> bool {
        let near_end = duration_ms.is_some_and(|d| position_ms.saturating_add(END_MARGIN_MS) >= d);
        if position_ms < MIN_BOOKMARK_MS || near_end {
            self.positions.remove(id);
            return false;
        }
        self.positions.insert(id.clone(), position_ms);
        true
    }

    pub fn get(&self, id: &TrackId) -> Option<u64> {
        self.positions.get(id).copied()
    }

    pub fn remove(&mut self, id: &TrackId) -> bool {
        self.positions.remove(id).is_some()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn clear(&mut self) {
        self.positions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_middle_positions_and_clears_edges() {
        let mut store = BookmarkStore::new();
        let id = TrackId::from("pod");
        assert!(store.save(&id, 60_000, Some(600_000)));
        assert_eq!(store.get(&id), Some(60_000));
        assert!(!store.save(&id, 2_000, Some(600_000)));
        assert_eq!(store.get(&id), None);
        assert!(store.save(&id, 100_000, None));
        assert!(!store.save(&id, 595_000, Some(600_000)));
        assert!(store.is_empty());
    }

    #[test]
    fn remove_and_clear() {
        let mut store = BookmarkStore::new();
        store.save(&"a".into(), 30_000, None);
        store.save(&"b".into(), 30_000, None);
        assert!(store.remove(&"a".into()));
        assert!(!store.remove(&"a".into()));
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty());
    }
}
