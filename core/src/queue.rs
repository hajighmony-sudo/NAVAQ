use crate::model::{PlayerError, RepeatMode, Track, TrackId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[error("queue snapshot contains duplicate track ids")]
    DuplicateTrackId,
    #[error("queue snapshot contains an invalid track")]
    InvalidTrack,
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
        if !snapshot.tracks.iter().all(Track::is_valid) {
            return Err(QueueSnapshotError::InvalidTrack);
        }
        let mut ids = HashSet::new();
        if !snapshot.tracks.iter().all(|track| ids.insert(&track.id)) {
            return Err(QueueSnapshotError::DuplicateTrackId);
        }
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

    pub fn contains(&self, id: &TrackId) -> bool {
        self.tracks.iter().any(|track| &track.id == id)
    }

    /// Replace the whole queue. Track ids must be unique; on error nothing changes.
    pub fn replace(&mut self, tracks: Vec<Track>, start_index: usize) -> Result<(), PlayerError> {
        if (tracks.is_empty() && start_index != 0)
            || (!tracks.is_empty() && start_index >= tracks.len())
        {
            return Err(PlayerError::InvalidQueueStartIndex);
        }
        if !tracks.iter().all(Track::is_valid) {
            return Err(PlayerError::InvalidTrack);
        }
        let mut ids = HashSet::new();
        if !tracks.iter().all(|track| ids.insert(&track.id)) {
            return Err(PlayerError::DuplicateTrack);
        }
        if tracks.is_empty() {
            self.reset();
            return Ok(());
        }
        self.tracks = tracks;
        self.current_index = Some(start_index);
        self.order = (0..self.tracks.len()).collect();
        self.cursor = Some(start_index);
        self.shuffle_enabled = false;
        Ok(())
    }

    /// Append a track at the end of the queue and of the playback order.
    pub fn enqueue(&mut self, track: Track) -> Result<(), PlayerError> {
        if !track.is_valid() {
            return Err(PlayerError::InvalidTrack);
        }
        if self.contains(&track.id) {
            return Err(PlayerError::DuplicateTrack);
        }
        let index = self.tracks.len();
        self.tracks.push(track);
        self.order.push(index);
        if self.current_index.is_none() {
            self.current_index = Some(index);
            self.cursor = Some(self.order.len() - 1);
        }
        Ok(())
    }

    /// Insert a track directly after the current one in both queue and playback order.
    pub fn enqueue_next(&mut self, track: Track) -> Result<(), PlayerError> {
        if !track.is_valid() {
            return Err(PlayerError::InvalidTrack);
        }
        if self.contains(&track.id) {
            return Err(PlayerError::DuplicateTrack);
        }
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
        Ok(())
    }

    /// Move a track to `new_index` in queue order. The current track stays selected.
    ///
    /// Without shuffle the playback order follows the new queue order. With shuffle the
    /// shuffled playback order is preserved; only the underlying queue order changes.
    pub fn move_track(&mut self, id: &TrackId, new_index: usize) -> Result<(), PlayerError> {
        let from = self
            .tracks
            .iter()
            .position(|track| &track.id == id)
            .ok_or(PlayerError::TrackNotFound)?;
        if new_index >= self.tracks.len() {
            return Err(PlayerError::InvalidQueueIndex);
        }
        if from == new_index {
            return Ok(());
        }

        let track = self.tracks.remove(from);
        self.tracks.insert(new_index, track);

        let remap = |index: usize| -> usize {
            if index == from {
                new_index
            } else if from < new_index && index > from && index <= new_index {
                index - 1
            } else if from > new_index && index >= new_index && index < from {
                index + 1
            } else {
                index
            }
        };

        self.current_index = self.current_index.map(remap);
        if self.shuffle_enabled {
            for ordered_index in &mut self.order {
                *ordered_index = remap(*ordered_index);
            }
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.cursor = self.current_index;
        }
        Ok(())
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

    /// Step back one position. At the start the current track is returned unchanged.
    pub fn previous(&mut self) -> Option<&Track> {
        self.previous_with_repeat(RepeatMode::Off)
    }

    /// Like `previous`, but wraps to the last track when repeat-all is active.
    pub fn previous_with_repeat(&mut self, repeat: RepeatMode) -> Option<&Track> {
        let cursor = self.cursor?;
        if cursor == 0 {
            if repeat != RepeatMode::All {
                return self.current();
            }
            self.cursor = Some(self.order.len() - 1);
        } else {
            self.cursor = Some(cursor - 1);
        }
        self.current_index = self.cursor.map(|position| self.order[position]);
        self.current()
    }

    /// The track that follows the current one on natural completion, without moving.
    /// Hosts can use this to preload the next item for gapless playback.
    pub fn peek_next(&self, repeat: RepeatMode) -> Option<&Track> {
        let cursor = self.cursor?;
        if repeat == RepeatMode::One {
            return self.current();
        }
        let position = if cursor + 1 < self.order.len() {
            cursor + 1
        } else if repeat == RepeatMode::All && !self.order.is_empty() {
            0
        } else {
            return None;
        };
        self.tracks.get(self.order[position])
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
            let seed = self
                .tracks
                .iter()
                .fold(0xcbf29ce484222325_u64, |hash, track| {
                    track.id.0.bytes().fold(hash, |value, byte| {
                        (value ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                    })
                });
            self.shuffle_after(cursor, seed);
        } else {
            self.order.sort_unstable();
        }

        self.cursor = self.order.iter().position(|item| *item == current_track);
        self.current_index = Some(current_track);
        self.shuffle_enabled = enabled;
    }

    /// Shuffle the tracks after the cursor with an xorshift generator seeded by `seed`.
    fn shuffle_after(&mut self, cursor: usize, seed: u64) {
        // xorshift gets stuck at zero, so map it to a fixed non-zero seed.
        let mut seed = if seed == 0 { 0x9e3779b97f4a7c15 } else { seed };
        for index in (cursor + 1..self.order.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let swap_index = cursor + 1 + (seed as usize % (index - cursor));
            self.order.swap(index, swap_index);
        }
    }

    /// Re-randomize the upcoming part of a shuffled queue. The current track and everything
    /// already played stay in place. Returns false when shuffle is off.
    pub fn reshuffle(&mut self, seed: u64) -> bool {
        match (self.shuffle_enabled, self.cursor) {
            (true, Some(cursor)) => {
                self.shuffle_after(cursor, seed);
                true
            }
            _ => false,
        }
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
    fn replacing_with_empty_tracks_clears_queue() {
        let mut queue = PlaybackQueue::new();
        queue.replace(vec![track("a"), track("b")], 1).unwrap();
        queue.replace(Vec::new(), 0).unwrap();

        assert!(queue.is_empty());
        assert!(queue.current().is_none());
        assert_eq!(queue.snapshot().cursor, None);
        assert!(queue.snapshot().order.is_empty());
        assert!(!queue.shuffle_enabled());
    }

    #[test]
    fn enqueue_and_advance() {
        let mut queue = PlaybackQueue::new();
        queue.enqueue(track("a")).unwrap();
        queue.enqueue(track("b")).unwrap();
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
        queue.enqueue_next(track("priority")).unwrap();
        assert_eq!(
            queue.next(RepeatMode::Off).unwrap().id,
            TrackId::from("priority")
        );
        assert!(queue.remove(&TrackId::from("b")));
        assert!(queue.current().is_some());
    }

    #[test]
    fn invalid_track_metadata_is_rejected_without_mutating_queue() {
        let mut queue = PlaybackQueue::new();
        queue.enqueue(track("valid")).unwrap();
        let mut invalid = track("invalid");
        invalid.title = "   ".into();
        assert_eq!(queue.enqueue(invalid.clone()), Err(PlayerError::InvalidTrack));
        assert_eq!(queue.enqueue_next(invalid), Err(PlayerError::InvalidTrack));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.current().unwrap().id, TrackId::from("valid"));
    }

    #[test]
    fn snapshot_restore_rejects_invalid_track_metadata() {
        let mut invalid = track("bad");
        invalid.source = AudioSource::LocalFile("".into());
        let snapshot = PlaybackQueueSnapshot {
            tracks: vec![invalid],
            order: vec![0],
            cursor: Some(0),
            shuffle_enabled: false,
        };
        assert_eq!(
            PlaybackQueue::from_snapshot(snapshot).unwrap_err(),
            QueueSnapshotError::InvalidTrack
        );
    }

    #[test]
    fn duplicate_ids_are_rejected_without_changing_the_queue() {
        let mut queue = PlaybackQueue::new();
        queue.replace(vec![track("a"), track("b")], 0).unwrap();
        assert_eq!(queue.enqueue(track("a")), Err(PlayerError::DuplicateTrack));
        assert_eq!(
            queue.enqueue_next(track("b")),
            Err(PlayerError::DuplicateTrack)
        );
        assert_eq!(
            queue.replace(vec![track("x"), track("x")], 0),
            Err(PlayerError::DuplicateTrack)
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.current().unwrap().id, TrackId::from("a"));
    }

    #[test]
    fn move_track_keeps_current_selected_and_updates_playback_order() {
        let mut queue = PlaybackQueue::new();
        queue
            .replace(vec![track("a"), track("b"), track("c"), track("d")], 1)
            .unwrap();
        queue.move_track(&TrackId::from("d"), 0).unwrap();
        let ids: Vec<_> = queue.tracks().iter().map(|t| t.id.0.clone()).collect();
        assert_eq!(ids, ["d", "a", "b", "c"]);
        assert_eq!(queue.current().unwrap().id, TrackId::from("b"));
        assert_eq!(queue.next(RepeatMode::Off).unwrap().id, TrackId::from("c"));

        queue.move_track(&TrackId::from("b"), 3).unwrap();
        assert_eq!(queue.current().unwrap().id, TrackId::from("c"));
        assert_eq!(queue.snapshot().order, vec![0, 1, 2, 3]);
        assert_eq!(
            queue.move_track(&TrackId::from("zzz"), 0),
            Err(PlayerError::TrackNotFound)
        );
        assert_eq!(
            queue.move_track(&TrackId::from("a"), 9),
            Err(PlayerError::InvalidQueueIndex)
        );
    }

    #[test]
    fn move_track_under_shuffle_preserves_playback_order() {
        let mut queue = PlaybackQueue::new();
        let tracks: Vec<_> = ["a", "b", "c", "d", "e", "f"].map(track).into();
        queue.replace(tracks, 0).unwrap();
        queue.set_shuffle(true);

        let playback_ids = |queue: &PlaybackQueue| -> Vec<String> {
            let snap = queue.snapshot();
            snap.order
                .iter()
                .map(|&i| snap.tracks[i].id.0.clone())
                .collect()
        };
        let before = playback_ids(&queue);
        queue.move_track(&TrackId::from("f"), 0).unwrap();
        queue.move_track(&TrackId::from("a"), 5).unwrap();
        assert_eq!(playback_ids(&queue), before);
        assert_eq!(queue.current().unwrap().id, TrackId::from("a"));

        queue.set_shuffle(false);
        let ids: Vec<_> = queue.tracks().iter().map(|t| t.id.0.clone()).collect();
        assert_eq!(ids, ["f", "b", "c", "d", "e", "a"]);
        assert_eq!(queue.current().unwrap().id, TrackId::from("a"));
    }

    #[test]
    fn previous_wraps_only_with_repeat_all_and_peek_does_not_move() {
        let mut queue = PlaybackQueue::new();
        queue.replace(vec![track("a"), track("b")], 0).unwrap();
        assert_eq!(queue.previous().unwrap().id, TrackId::from("a"));
        assert_eq!(
            queue.previous_with_repeat(RepeatMode::All).unwrap().id,
            TrackId::from("b")
        );
        assert!(queue.peek_next(RepeatMode::Off).is_none());
        assert_eq!(
            queue.peek_next(RepeatMode::All).unwrap().id,
            TrackId::from("a")
        );
        assert_eq!(
            queue.peek_next(RepeatMode::One).unwrap().id,
            TrackId::from("b")
        );
        assert_eq!(queue.current().unwrap().id, TrackId::from("b"));
    }

    #[test]
    fn snapshot_with_duplicate_ids_is_rejected() {
        let snapshot = PlaybackQueueSnapshot {
            tracks: vec![track("a"), track("a")],
            order: vec![0, 1],
            cursor: Some(0),
            shuffle_enabled: false,
        };
        assert_eq!(
            PlaybackQueue::from_snapshot(snapshot).unwrap_err(),
            QueueSnapshotError::DuplicateTrackId
        );
    }

    #[test]
    fn reshuffle_changes_upcoming_order_but_keeps_played_and_current() {
        let mut queue = PlaybackQueue::new();
        let tracks: Vec<_> = (0..12).map(|i| track(&format!("t{i}"))).collect();
        queue.replace(tracks, 3).unwrap();
        assert!(!queue.reshuffle(1));
        queue.set_shuffle(true);
        let before = queue.snapshot();
        assert!(queue.reshuffle(0xdead_beef));
        let after = queue.snapshot();
        assert_eq!(before.order[..=3], after.order[..=3]);
        assert_ne!(before.order[4..], after.order[4..]);
        let mut sorted = after.order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..12).collect::<Vec<_>>());
        assert!(queue.reshuffle(0));
    }
}
