use crate::model::{PlayerError, RepeatMode, Track, TrackId};

#[derive(Debug, Clone, Default)]
pub struct PlaybackQueue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
}

impl PlaybackQueue {
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
        Ok(())
    }

    pub fn enqueue(&mut self, track: Track) {
        self.tracks.push(track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
    }

    pub fn enqueue_next(&mut self, track: Track) {
        match self.current_index {
            Some(index) => self.tracks.insert(index + 1, track),
            None => {
                self.tracks.push(track);
                self.current_index = Some(0);
            }
        }
    }

    pub fn remove(&mut self, id: &TrackId) -> bool {
        let Some(index) = self.tracks.iter().position(|track| &track.id == id) else {
            return false;
        };

        self.tracks.remove(index);
        self.current_index = match self.current_index {
            None => None,
            Some(_) if self.tracks.is_empty() => None,
            Some(current) if index < current => Some(current - 1),
            Some(current) if index == current => Some(current.min(self.tracks.len() - 1)),
            Some(current) => Some(current),
        };
        true
    }

    pub fn select(&mut self, id: &TrackId) -> Result<&Track, PlayerError> {
        let index = self
            .tracks
            .iter()
            .position(|track| &track.id == id)
            .ok_or(PlayerError::TrackNotFound)?;
        self.current_index = Some(index);
        Ok(&self.tracks[index])
    }

    pub fn next(&mut self, repeat: RepeatMode) -> Option<&Track> {
        let current = self.current_index?;
        let next = current + 1;

        if next < self.tracks.len() {
            self.current_index = Some(next);
        } else if repeat == RepeatMode::All && !self.tracks.is_empty() {
            self.current_index = Some(0);
        } else {
            return None;
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<&Track> {
        let current = self.current_index?;
        if current == 0 {
            return self.current();
        }
        self.current_index = Some(current - 1);
        self.current()
    }

    pub fn reset(&mut self) {
        self.tracks.clear();
        self.current_index = None;
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
}
