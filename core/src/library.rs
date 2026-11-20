//! In-memory music library: unique tracks with search, sorting and grouping.
//! Scanning files and storage are host responsibilities; the host feeds tracks in.

use crate::model::{Track, TrackId};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSummary {
    pub name: String,
    pub artist: Option<String>,
    pub track_ids: Vec<TrackId>,
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    tracks: Vec<Track>,
    index: HashMap<TrackId, usize>,
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a library from stored tracks. A later track with an existing id replaces it.
    pub fn from_tracks(tracks: impl IntoIterator<Item = Track>) -> Self {
        let mut library = Self::new();
        for track in tracks {
            library.upsert(track);
        }
        library
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn get(&self, id: &TrackId) -> Option<&Track> {
        self.index.get(id).map(|&i| &self.tracks[i])
    }

    /// Insert a track, or replace the one with the same id. Returns true if it was new.
    pub fn upsert(&mut self, track: Track) -> bool {
        match self.index.get(&track.id) {
            Some(&i) => {
                self.tracks[i] = track;
                false
            }
            None => {
                self.index.insert(track.id.clone(), self.tracks.len());
                self.tracks.push(track);
                true
            }
        }
    }

    pub fn remove(&mut self, id: &TrackId) -> bool {
        let Some(i) = self.index.remove(id) else {
            return false;
        };
        self.tracks.remove(i);
        for slot in self.index.values_mut() {
            if *slot > i {
                *slot -= 1;
            }
        }
        true
    }

    /// Case-insensitive search over title, artist and album. Every whitespace-separated
    /// word of the query must match somewhere. An empty query matches nothing.
    pub fn search(&self, query: &str) -> Vec<&Track> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        self.tracks
            .iter()
            .filter(|track| {
                let haystack = format!(
                    "{} {} {}",
                    track.title,
                    track.artist.as_deref().unwrap_or(""),
                    track.album.as_deref().unwrap_or("")
                )
                .to_lowercase();
                words.iter().all(|word| haystack.contains(word))
            })
            .collect()
    }

    /// Tracks ordered by `key` (case-insensitive; missing values last; ties by title, id).
    pub fn sorted(&self, key: SortKey) -> Vec<&Track> {
        let text = |value: Option<&str>| (value.is_none(), value.unwrap_or("").to_lowercase());
        let mut tracks: Vec<&Track> = self.tracks.iter().collect();
        tracks.sort_by(|a, b| {
            let primary = match key {
                SortKey::Title => text(Some(&a.title)).cmp(&text(Some(&b.title))),
                SortKey::Artist => text(a.artist.as_deref()).cmp(&text(b.artist.as_deref())),
                SortKey::Album => text(a.album.as_deref()).cmp(&text(b.album.as_deref())),
                SortKey::Duration => (a.duration_ms.is_none(), a.duration_ms)
                    .cmp(&(b.duration_ms.is_none(), b.duration_ms)),
            };
            primary
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                .then_with(|| a.id.0.cmp(&b.id.0))
        });
        tracks
    }

    /// Albums grouped by (album, artist), ordered by name; tracks without an album are skipped.
    pub fn albums(&self) -> Vec<AlbumSummary> {
        let mut groups: BTreeMap<(String, Option<String>), Vec<TrackId>> = BTreeMap::new();
        for track in &self.tracks {
            if let Some(album) = &track.album {
                groups
                    .entry((album.clone(), track.artist.clone()))
                    .or_default()
                    .push(track.id.clone());
            }
        }
        groups
            .into_iter()
            .map(|((name, artist), track_ids)| AlbumSummary {
                name,
                artist,
                track_ids,
            })
            .collect()
    }

    /// Distinct artist names, sorted.
    pub fn artists(&self) -> Vec<String> {
        let mut artists: Vec<String> = self
            .tracks
            .iter()
            .filter_map(|track| track.artist.clone())
            .collect();
        artists.sort_by_key(|name| name.to_lowercase());
        artists.dedup();
        artists
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;

    fn t(
        id: &str,
        title: &str,
        artist: Option<&str>,
        album: Option<&str>,
        dur: Option<u64>,
    ) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.map(Into::into),
            album: album.map(Into::into),
            duration_ms: dur,
            source: AudioSource::LocalFile(format!("/{id}.mp3")),
        }
    }

    fn library() -> Library {
        Library::from_tracks([
            t("1", "Beta", Some("Zed"), Some("Alpha Album"), Some(200)),
            t("2", "alpha", Some("Amy"), Some("Alpha Album"), Some(100)),
            t("3", "Gamma", None, None, None),
            t("4", "سلام", Some("علی"), Some("آلبوم"), Some(300)),
        ])
    }

    #[test]
    fn upsert_replaces_by_id_and_remove_keeps_index_consistent() {
        let mut lib = library();
        assert!(!lib.upsert(t("2", "Renamed", None, None, None)));
        assert_eq!(lib.get(&"2".into()).unwrap().title, "Renamed");
        assert!(lib.remove(&"1".into()));
        assert!(!lib.remove(&"1".into()));
        assert_eq!(lib.get(&"4".into()).unwrap().title, "سلام");
        assert_eq!(lib.len(), 3);
    }

    #[test]
    fn search_is_case_insensitive_multiword_and_unicode() {
        let lib = library();
        assert_eq!(lib.search("ALPHA").len(), 2);
        assert_eq!(lib.search("alpha amy").len(), 1);
        assert_eq!(lib.search("علی")[0].id, TrackId::from("4"));
        assert!(lib.search("   ").is_empty());
        assert!(lib.search("nothing").is_empty());
    }

    #[test]
    fn sorting_puts_missing_values_last() {
        let lib = library();
        let ids = |key| -> Vec<String> { lib.sorted(key).iter().map(|t| t.id.0.clone()).collect() };
        assert_eq!(ids(SortKey::Title), ["2", "1", "3", "4"]);
        assert_eq!(ids(SortKey::Artist), ["2", "1", "4", "3"]);
        assert_eq!(ids(SortKey::Duration), ["2", "1", "4", "3"]);
    }

    #[test]
    fn groups_albums_and_artists() {
        let lib = library();
        let albums = lib.albums();
        assert_eq!(albums.len(), 3);
        assert_eq!(albums[0].name, "Alpha Album");
        assert_eq!(lib.artists(), ["Amy", "Zed", "علی"]);
    }
}
