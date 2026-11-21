//! User playlists: ordered lists of unique track ids, resolved against a `Library`.

use crate::library::Library;
use crate::model::{Track, TrackId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub track_ids: Vec<TrackId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlaylistError {
    #[error("playlist was not found")]
    NotFound,
    #[error("playlist name must not be empty")]
    EmptyName,
    #[error("track is already in the playlist")]
    DuplicateTrack,
    #[error("track is not in the playlist")]
    TrackNotInPlaylist,
    #[error("playlist index is out of range")]
    InvalidIndex,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistStore {
    playlists: Vec<Playlist>,
    next_id: u64,
}

impl PlaylistStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn get(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Playlist, PlaylistError> {
        self.playlists
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(PlaylistError::NotFound)
    }

    /// Create an empty playlist and return its generated id.
    pub fn create(&mut self, name: &str) -> Result<String, PlaylistError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PlaylistError::EmptyName);
        }
        let id = loop {
            self.next_id += 1;
            let candidate = format!("pl-{}", self.next_id);
            if self.get(&candidate).is_none() {
                break candidate;
            }
        };
        self.playlists.push(Playlist {
            id: id.clone(),
            name: name.to_owned(),
            track_ids: Vec::new(),
        });
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), PlaylistError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PlaylistError::EmptyName);
        }
        self.get_mut(id)?.name = name.to_owned();
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), PlaylistError> {
        let before = self.playlists.len();
        self.playlists.retain(|p| p.id != id);
        if self.playlists.len() == before {
            Err(PlaylistError::NotFound)
        } else {
            Ok(())
        }
    }

    pub fn add_track(&mut self, id: &str, track: TrackId) -> Result<(), PlaylistError> {
        let playlist = self.get_mut(id)?;
        if playlist.track_ids.contains(&track) {
            return Err(PlaylistError::DuplicateTrack);
        }
        playlist.track_ids.push(track);
        Ok(())
    }

    pub fn remove_track(&mut self, id: &str, track: &TrackId) -> Result<(), PlaylistError> {
        let playlist = self.get_mut(id)?;
        let position = playlist
            .track_ids
            .iter()
            .position(|t| t == track)
            .ok_or(PlaylistError::TrackNotInPlaylist)?;
        playlist.track_ids.remove(position);
        Ok(())
    }

    pub fn move_track(
        &mut self,
        id: &str,
        track: &TrackId,
        new_index: usize,
    ) -> Result<(), PlaylistError> {
        let playlist = self.get_mut(id)?;
        let from = playlist
            .track_ids
            .iter()
            .position(|t| t == track)
            .ok_or(PlaylistError::TrackNotInPlaylist)?;
        if new_index >= playlist.track_ids.len() {
            return Err(PlaylistError::InvalidIndex);
        }
        let item = playlist.track_ids.remove(from);
        playlist.track_ids.insert(new_index, item);
        Ok(())
    }

    /// Drop a track from every playlist (call when it leaves the library).
    pub fn remove_track_everywhere(&mut self, track: &TrackId) {
        for playlist in &mut self.playlists {
            playlist.track_ids.retain(|t| t != track);
        }
    }

    /// Playlist tracks in order, skipping ids that are no longer in the library.
    /// The result can be passed straight to `PlayerCommand::SetQueue`.
    pub fn resolve(&self, id: &str, library: &Library) -> Result<Vec<Track>, PlaylistError> {
        let playlist = self.get(id).ok_or(PlaylistError::NotFound)?;
        Ok(playlist
            .track_ids
            .iter()
            .filter_map(|t| library.get(t).cloned())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            album: None,
            duration_ms: None,
            source: AudioSource::LocalFile(format!("/{id}.mp3")),
        }
    }

    #[test]
    fn create_rename_delete() {
        let mut store = PlaylistStore::new();
        assert_eq!(store.create("  "), Err(PlaylistError::EmptyName));
        let a = store.create(" Road ").unwrap();
        let b = store.create("Gym").unwrap();
        assert_ne!(a, b);
        assert_eq!(store.get(&a).unwrap().name, "Road");
        store.rename(&a, "Trip").unwrap();
        assert_eq!(store.get(&a).unwrap().name, "Trip");
        store.delete(&a).unwrap();
        assert_eq!(store.delete(&a), Err(PlaylistError::NotFound));
        // ids are never reused after a delete
        assert_ne!(store.create("New").unwrap(), a);
    }

    #[test]
    fn add_remove_move_tracks() {
        let mut store = PlaylistStore::new();
        let id = store.create("P").unwrap();
        for t in ["a", "b", "c"] {
            store.add_track(&id, t.into()).unwrap();
        }
        assert_eq!(
            store.add_track(&id, "a".into()),
            Err(PlaylistError::DuplicateTrack)
        );
        store.move_track(&id, &"c".into(), 0).unwrap();
        let order: Vec<_> = store
            .get(&id)
            .unwrap()
            .track_ids
            .iter()
            .map(|t| t.0.clone())
            .collect();
        assert_eq!(order, ["c", "a", "b"]);
        assert_eq!(
            store.move_track(&id, &"a".into(), 9),
            Err(PlaylistError::InvalidIndex)
        );
        store.remove_track(&id, &"a".into()).unwrap();
        assert_eq!(
            store.remove_track(&id, &"a".into()),
            Err(PlaylistError::TrackNotInPlaylist)
        );
        store.remove_track_everywhere(&"c".into());
        assert_eq!(store.get(&id).unwrap().track_ids.len(), 1);
    }

    #[test]
    fn resolve_skips_tracks_missing_from_library() {
        let mut store = PlaylistStore::new();
        let id = store.create("P").unwrap();
        store.add_track(&id, "a".into()).unwrap();
        store.add_track(&id, "gone".into()).unwrap();
        store.add_track(&id, "b".into()).unwrap();
        let library = Library::from_tracks([track("a"), track("b")]);
        let ids: Vec<_> = store
            .resolve(&id, &library)
            .unwrap()
            .into_iter()
            .map(|t| t.id.0)
            .collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(
            store.resolve("nope", &library),
            Err(PlaylistError::NotFound)
        );
    }
}
