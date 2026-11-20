use crate::model::{PlayerError, RepeatMode, Track, TrackId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct PlaybackQueue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    order: Vec<usize>,
    cursor: Option<usize>,
    shuffle_enabled: bool,
}

/// Serializable queue state, including the effective shuffled playback order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackQueueSnapshot {
    pub tracks: Vec<Track>,
    pub order: Vec<usize>,
    pub cursor: Option<usize>,
    pub shuffle_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueueSnapshotError {
    #[error("queue snapshot order must contain each track index exactly once")]
    InvalidOrder,
    #[error("queue snapshot cursor is outside the playback order")]
    InvalidCursor,
}

impl PlaybackQueue {
    /// Capture all queue ordering state needed for deterministic restoration.
    pub fn snapshot(&self) -> PlaybackQueueSnapshot {
        PlaybackQueueSnapshot {
            tracks: self.tracks.clone(),
            order: self.order.clone(),
            cursor: self.cursor,
            shuffle_enabled: self.shuffle_enabled,
        }
    }

    /// Restore a queue after validating its internal index invariants.
    pub fn from_snapshot(snapshot: PlaybackQueueSnapshot) -> Result<Self, QueueSnapshotError> {
        let mut seen = vec![false; snapshot.tracks.len()];
        if snapshot.order.len() != snapshot.tracks.len() {
            return Err(QueueSnapshotError::InvalidOrder);
        }
        for index in &snapshot.order {
            let Some(slot) = seen.get_mut(*index) else {
                return Err(QueueSnapshotError::InvalidOrder);
            };
            if *slot {
                return Err(QueueSnapshotError::InvalidOrder);
            }
            *slot = true;
        }

        let current_index = match snapshot.cursor {
            Some(cursor) => Some(
                *snapshot
                    .order
                    .get(cursor)
                    .ok_or(QueueSnapshotError::InvalidCursor)?,
            ),
            None if snapshot.order.is_empty() => None,
            None => return Err(QueueSnapshotError::InvalidCursor),
        };

        Ok(Self {
            tracks: snapshot.tracks,
            current_index,
            order: snapshot.order,
            cursor: snapshot.cursor,
            shuffle_enabled: snapshot.shuffle_enabled,
        })
    }
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index.and_then(|index| self.tracks.get(index))
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn replace(&mut self, tracks: Vec<Track>, start_index: usize) -> Result<(), PlayerError> {
        if tracks.is_empty() || start_index >= tracks.len() {
            return Err(PlayerError::InvalidQueueStartIndex);
        }
        self.tracks = tracks;
        self.current_index = Some(start_index);
        self.order = (0..self.tracks.len()).collect();
        self.cursor = Some(start_index);
        self.shuffle_enabled = false;
        Ok(())
    }

    pub fn enqueue(&mut self, track: Track) {
        let index = self.tracks.len();
        self.tracks.push(track);
        self.order.push(index);
        if self.current_index.is_none() {
            self.current_index = Some(index);
            self.cursor = Some(self.order.len() - 1);
        }
    }

    pub fn enqueue_next(&mut self, track: Track) {
        match self.current_index {
            Some(index) => {
                let insert_at = index + 1;
                self.tracks.insert(insert_at, track);
                for ordered_index in &mut self.order {
                    if *ordered_index >= insert_at {
                        *ordered_index += 1;
                    }
                }
                let cursor = self.cursor.expect("current queue item has a cursor");
                self.order.insert(cursor + 1, insert_at);
            }
            None => {
                self.tracks.push(track);
                self.order = vec![0];
                self.current_index = Some(0);
                self.cursor = Some(0);
            }
        }
    }

    pub fn remove(&mut self, id: &TrackId) -> bool {
        let Some(index) = self.tracks.iter().position(|track| &track.id == id) else {
            return false;
        };

        self.tracks.remove(index);
        let old_cursor = self.cursor;
        let removed_order_position = self.order.iter().position(|item| *item == index);
        self.order.retain(|item| *item != index);
        for ordered_index in &mut self.order {
            if *ordered_index > index {
                *ordered_index -= 1;
            }
        }
        self.cursor = match (old_cursor, removed_order_position, self.order.len()) {
            (_, _, 0) => None,
            (Some(cursor), Some(removed), _) if cursor > removed => Some(cursor - 1),
            (Some(cursor), Some(removed), _) if cursor == removed => {
                Some(cursor.min(self.order.len() - 1))
            }
            (Some(cursor), _, _) => Some(cursor),
            _ => None,
        };
        self.current_index = self.cursor.map(|cursor| self.order[cursor]);
        true
    }

    pub fn select(&mut self, id: &TrackId) -> Result<&Track, PlayerError> {
        let index = self
            .tracks
            .iter()
            .position(|track| &track.id == id)
            .ok_or(PlayerError::TrackNotFound)?;
        self.current_index = Some(index);
        self.cursor = self.order.iter().position(|item| *item == index);
        Ok(&self.tracks[index])
    }

    pub fn next(&mut self, repeat: RepeatMode) -> Option<&Track> {
        let cursor = self.cursor?;
        let next = cursor + 1;

        if next < self.order.len() {
            self.cursor = Some(next);
        } else if repeat == RepeatMode::All && !self.order.is_empty() {
            self.cursor = Some(0);
        } else {
            return None;
        }
        self.current_index = self.cursor.map(|position| self.order[position]);
        self.current()
    }

    pub fn previous(&mut self) -> Option<&Track> {
        let cursor = self.cursor?;
        if cursor == 0 {
            return self.current();
        }
        self.cursor = Some(cursor - 1);
        self.current_index = self.cursor.map(|position| self.order[position]);
        self.current()
    }

    /// Toggle deterministic shuffle while preserving the currently selected track.
    /// The current item stays in place; only the remaining queue is reordered.
    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle_enabled == enabled || self.order.len() < 2 {
            self.shuffle_enabled = enabled;
            return;
        }

        let Some(cursor) = self.cursor else {
            self.shuffle_enabled = enabled;
            return;
        };

        let current_track = self.order[cursor];
        if enabled {
            let mut seed = self
                .tracks
                .iter()
                .fold(0xcbf29ce484222325_u64, |hash, track| {
                    track.id.0.bytes().fold(hash, |value, byte| {
                        (value ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                    })
                });
            for index in (cursor + 1..self.order.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let swap_index = cursor + 1 + (seed as usize % (index - cursor));
                self.order.swap(index, swap_index);
            }
        } else {
            self.order.sort_unstable();
        }

        self.cursor = self.order.iter().position(|item| *item == current_track);
        self.current_index = Some(current_track);
        self.shuffle_enabled = enabled;
    }

    pub fn shuffle_enabled(&self) -> bool {
        self.shuffle_enabled
    }

    pub fn reset(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.order.clear();
        self.cursor = None;
        self.shuffle_enabled = false;
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
            duration_ms: Some(1000),
            source: AudioSource::LocalFile(format!("/{id}.mp3")),
        }
    }

    #[test]
    fn enqueue_and_advance() {
        let mut queue = PlaybackQueue::new();
        queue.enqueue(track("a"));
        queue.enqueue(track("b"));
        assert_eq!(queue.current().unwrap().id, TrackId::from("a"));
        assert_eq!(queue.next(RepeatMode::Off).unwrap().id, TrackId::from("b"));
        assert!(queue.next(RepeatMode::Off).is_none());
    }

    #[test]
    fn repeat_all_wraps_to_first_track() {
        let mut queue = PlaybackQueue::new();
        queue.replace(vec![track("a"), track("b")], 1).unwrap();
        assert_eq!(queue.next(RepeatMode::All).unwrap().id, TrackId::from("a"));
    }

    #[test]
    fn removing_before_current_keeps_current_track_selected() {
        let mut queue = PlaybackQueue::new();
        queue
            .replace(vec![track("a"), track("b"), track("c")], 2)
            .unwrap();
        assert!(queue.remove(&TrackId::from("a")));
        assert_eq!(queue.current().unwrap().id, TrackId::from("c"));
    }

    #[test]
    fn shuffle_preserves_current_track_and_disabling_restores_queue_order() {
        let mut queue = PlaybackQueue::new();
        queue
            .replace(
                vec![track("a"), track("b"), track("c"), track("d"), track("e")],
                2,
            )
            .unwrap();
        queue.set_shuffle(true);
        assert_eq!(queue.current().unwrap().id, TrackId::from("c"));
        assert!(queue.shuffle_enabled());
        queue.set_shuffle(false);
        assert_eq!(queue.current().unwrap().id, TrackId::from("c"));
        assert_eq!(queue.next(RepeatMode::Off).unwrap().id, TrackId::from("d"));
        assert_eq!(queue.next(RepeatMode::Off).unwrap().id, TrackId::from("e"));
        assert!(!queue.shuffle_enabled());
    }

    #[test]
    fn play_next_and_remove_keep_shuffle_order_indices_valid() {
        let mut queue = PlaybackQueue::new();
        queue
            .replace(vec![track("a"), track("b"), track("c")], 0)
            .unwrap();
        queue.set_shuffle(true);
        queue.enqueue_next(track("priority"));
        assert_eq!(
            queue.next(RepeatMode::Off).unwrap().id,
            TrackId::from("priority")
        );
        assert!(queue.remove(&TrackId::from("b")));
        assert!(queue.current().is_some());
    }
}
