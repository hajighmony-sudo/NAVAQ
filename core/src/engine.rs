use crate::model::{
    PlaybackState, PlaybackStatus, PlayerCommand, PlayerError, PlayerEvent, RepeatMode, TrackId,
};
use crate::queue::PlaybackQueue;

/// Deterministic, platform-independent playback coordinator.
///
/// It owns domain state only. Actual decoding, audio output, buffering and OS media sessions
/// belong to the Android/iOS adapters and report their results back through adapter callbacks.
#[derive(Debug, Default)]
pub struct PlayerCore {
    state: PlaybackState,
    queue: PlaybackQueue,
}

impl PlayerCore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    pub fn queue(&self) -> &PlaybackQueue {
        &self.queue
    }

    pub fn handle(&mut self, command: PlayerCommand) -> Result<Vec<PlayerEvent>, PlayerError> {
        let mut events = Vec::new();

        match command {
            PlayerCommand::Play | PlayerCommand::Resume => {
                if self.queue.current().is_none() {
                    return Err(PlayerError::EmptyQueue);
                }
                self.state.status = PlaybackStatus::Playing;
                events.push(PlayerEvent::PlaybackStarted);
            }
            PlayerCommand::Pause => {
                if self.state.status == PlaybackStatus::Playing {
                    self.state.status = PlaybackStatus::Paused;
                    events.push(PlayerEvent::PlaybackPaused);
                }
            }
            PlayerCommand::Stop => {
                self.state.status = PlaybackStatus::Stopped;
                self.state.position_ms = 0;
                self.state.is_buffering = false;
                events.push(PlayerEvent::PlaybackStopped);
            }
            PlayerCommand::Seek { position_ms } => {
                self.state.position_ms = self.state.duration_ms
                    .map_or(position_ms, |duration| position_ms.min(duration));
            }
            PlayerCommand::Next => {
                if self.state.repeat_mode == RepeatMode::One {
                    self.state.position_ms = 0;
                } else if self.queue.next(self.state.repeat_mode).is_some() {
                    self.sync_track_from_queue();
                    events.push(PlayerEvent::TrackChanged(self.state.current_track.clone()));
                    self.state.status = PlaybackStatus::Playing;
                } else {
                    self.state.status = PlaybackStatus::Completed;
                    events.push(PlayerEvent::PlaybackCompleted);
                }
            }
            PlayerCommand::Previous => {
                if self.state.position_ms > 3_000 {
                    self.state.position_ms = 0;
                } else if self.queue.previous().is_some() {
                    self.sync_track_from_queue();
                    events.push(PlayerEvent::TrackChanged(self.state.current_track.clone()));
                    self.state.status = PlaybackStatus::Playing;
                }
            }
            PlayerCommand::PlayTrack(id) => {
                self.queue.select(&id)?;
                self.sync_track_from_queue();
                self.state.status = PlaybackStatus::Playing;
                events.push(PlayerEvent::TrackChanged(Some(id)));
                events.push(PlayerEvent::PlaybackStarted);
            }
            PlayerCommand::AddToQueue(track) => {
                self.queue.enqueue(track);
                events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::PlayNext(track) => {
                self.queue.enqueue_next(track);
                events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::RemoveFromQueue(id) => {
                if self.queue.remove(&id) {
                    self.sync_track_from_queue();
                    events.push(PlayerEvent::QueueChanged);
                    events.push(PlayerEvent::TrackChanged(self.state.current_track.clone()));
                }
            }
            PlayerCommand::SetVolume(volume) => {
                if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
                    return Err(PlayerError::InvalidVolume);
                }
                self.state.volume = volume;
            }
            PlayerCommand::SetSpeed(speed) => {
                if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
                    return Err(PlayerError::InvalidSpeed);
                }
                self.state.speed = speed;
            }
            PlayerCommand::SetRepeatMode(mode) => self.state.repeat_mode = mode,
            PlayerCommand::SetShuffle(enabled) => self.state.shuffle_enabled = enabled,
        }

        events.push(PlayerEvent::StateChanged(self.state.clone()));
        Ok(events)
    }

    /// Called by a native backend when it has prepared a media item.
    pub fn on_prepared(&mut self, duration_ms: Option<u64>) -> PlayerEvent {
        self.state.duration_ms = duration_ms;
        self.state.position_ms = 0;
        self.state.status = PlaybackStatus::Ready;
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Called by a native backend for periodic playback position updates.
    pub fn on_position(&mut self, position_ms: u64) -> PlayerEvent {
        self.state.position_ms = self.state.duration_ms
            .map_or(position_ms, |duration| position_ms.min(duration));
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Called by a native backend when buffering begins or ends.
    pub fn on_buffering(&mut self, buffering: bool) -> PlayerEvent {
        self.state.is_buffering = buffering;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Called by a native backend when decoding or output fails.
    pub fn on_error(&mut self) -> PlayerEvent {
        self.state.status = PlaybackStatus::Error;
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    fn sync_track_from_queue(&mut self) {
        if let Some(track) = self.queue.current() {
            self.state.current_track = Some(track.id.clone());
            self.state.duration_ms = track.duration_ms;
        } else {
            self.state.current_track = None;
            self.state.duration_ms = None;
            self.state.position_ms = 0;
            self.state.status = PlaybackStatus::Idle;
        }
        self.state.position_ms = 0;
    }

    pub fn current_track_id(&self) -> Option<&TrackId> {
        self.state.current_track.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioSource, Track};

    fn track(id: &str, duration_ms: u64) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            album: None,
            duration_ms: Some(duration_ms),
            source: AudioSource::LocalFile(format!("/{id}.flac")),
        }
    }

    #[test]
    fn play_track_updates_state_and_emits_event() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 12_000))).unwrap();
        let events = core.handle(PlayerCommand::Play).unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert!(events.iter().any(|event| matches!(event, PlayerEvent::PlaybackStarted)));
    }

    #[test]
    fn volume_and_speed_are_validated() {
        let mut core = PlayerCore::new();
        assert_eq!(core.handle(PlayerCommand::SetVolume(1.5)), Err(PlayerError::InvalidVolume));
        assert_eq!(core.handle(PlayerCommand::SetSpeed(0.0)), Err(PlayerError::InvalidSpeed));
    }

    #[test]
    fn seek_is_clamped_to_duration() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 5_000))).unwrap();
        core.handle(PlayerCommand::PlayTrack(TrackId::from("one"))).unwrap();
        core.on_prepared(Some(5_000));
        core.handle(PlayerCommand::Seek { position_ms: 9_000 }).unwrap();
        assert_eq!(core.state().position_ms, 5_000);
    }
}
