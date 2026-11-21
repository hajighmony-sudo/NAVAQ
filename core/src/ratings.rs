//! Favorites and 1-5 star ratings keyed by track id.

use crate::library::Library;
use crate::model::{Track, TrackId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("rating must be between 1 and 5")]
pub struct RatingOutOfRange;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserRatings {
    favorites: BTreeSet<TrackId>,
    ratings: BTreeMap<TrackId, u8>,
}

impl UserRatings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_favorite(&mut self, id: TrackId, favorite: bool) {
        if favorite {
            self.favorites.insert(id);
        } else {
            self.favorites.remove(&id);
        }
    }

    /// Flip the favorite flag and return the new value.
    pub fn toggle_favorite(&mut self, id: TrackId) -> bool {
        let now = !self.favorites.contains(&id);
        self.set_favorite(id, now);
        now
    }

    pub fn is_favorite(&self, id: &TrackId) -> bool {
        self.favorites.contains(id)
    }

    pub fn favorites(&self) -> impl Iterator<Item = &TrackId> {
        self.favorites.iter()
    }

    /// Set a 1-5 rating, or clear it with `None`.
    pub fn set_rating(&mut self, id: TrackId, rating: Option<u8>) -> Result<(), RatingOutOfRange> {
        match rating {
            None => {
                self.ratings.remove(&id);
            }
            Some(value) if (1..=5).contains(&value) => {
                self.ratings.insert(id, value);
            }
            Some(_) => return Err(RatingOutOfRange),
        }
        Ok(())
    }

    pub fn rating(&self, id: &TrackId) -> Option<u8> {
        self.ratings.get(id).copied()
    }

    /// Track ids rated at least `min_rating`, best first (ties by id).
    pub fn rated_at_least(&self, min_rating: u8) -> Vec<&TrackId> {
        let mut found: Vec<_> = self
            .ratings
            .iter()
            .filter(|(_, &r)| r >= min_rating)
            .collect();
        found.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        found.into_iter().map(|(id, _)| id).collect()
    }

    /// Favorite tracks that still exist in the library, ready for `SetQueue`.
    pub fn favorite_tracks(&self, library: &Library) -> Vec<Track> {
        self.favorites
            .iter()
            .filter_map(|id| library.get(id).cloned())
            .collect()
    }

    /// Forget everything about a track (for example when it leaves the library).
    pub fn forget(&mut self, id: &TrackId) {
        self.favorites.remove(id);
        self.ratings.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;

    #[test]
    fn favorites_toggle_and_forget() {
        let mut r = UserRatings::new();
        assert!(r.toggle_favorite("a".into()));
        assert!(r.is_favorite(&"a".into()));
        assert!(!r.toggle_favorite("a".into()));
        r.set_favorite("b".into(), true);
        r.forget(&"b".into());
        assert_eq!(r.favorites().count(), 0);
    }

    #[test]
    fn ratings_are_validated_and_sorted() {
        let mut r = UserRatings::new();
        assert_eq!(r.set_rating("a".into(), Some(0)), Err(RatingOutOfRange));
        assert_eq!(r.set_rating("a".into(), Some(6)), Err(RatingOutOfRange));
        r.set_rating("a".into(), Some(4)).unwrap();
        r.set_rating("b".into(), Some(5)).unwrap();
        r.set_rating("c".into(), Some(2)).unwrap();
        let top: Vec<_> = r
            .rated_at_least(4)
            .into_iter()
            .map(|t| t.0.as_str())
            .collect();
        assert_eq!(top, ["b", "a"]);
        r.set_rating("b".into(), None).unwrap();
        assert_eq!(r.rating(&"b".into()), None);
    }

    #[test]
    fn favorite_tracks_skip_missing_library_entries() {
        let track = Track {
            id: "a".into(),
            title: "A".into(),
            artist: None,
            album: None,
            duration_ms: None,
            source: AudioSource::LocalFile("/a.mp3".into()),
        };
        let library = Library::from_tracks([track]);
        let mut r = UserRatings::new();
        r.set_favorite("a".into(), true);
        r.set_favorite("gone".into(), true);
        assert_eq!(r.favorite_tracks(&library).len(), 1);
    }
}
