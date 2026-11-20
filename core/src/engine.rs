use crate::model::{
    BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus, PlayerCommand,
    PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
use crate::persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
use crate::queue::PlaybackQueue;

/// Platform-independent playback coordinator.
///
/// The core owns queue and playback state. It emits explicit backend actions; native
/// Android/iOS adapters execute those actions using Media3 or AVFoundation respectively.
#[derive(Debug, Default)]
pub struct PlayerCore {
    state: PlaybackState,
    queue: PlaybackQueue,
    backend_loaded_track: Option<TrackId>,
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

    pub fn handle(&mut self, command: PlayerCommand) -> Result<CoreOutput, PlayerError> {
        let mut output = CoreOutput::default();

        match command {
            PlayerCommand::Play | PlayerCommand::Resume => {
                let track = self
                    .queue
                    .current()
                    .cloned()
                    .ok_or(PlayerError::EmptyQueue)?;
                self.sync_selected_track(&track, &mut output, true);
                output.backend_actions.push(BackendAction::Play);
                self.state.status = PlaybackStatus::Playing;
                output.events.push(PlayerEvent::PlaybackStarted);
            }
            PlayerCommand::Pause => {
                if self.state.status == PlaybackStatus::Playing {
                    output.backend_actions.push(BackendAction::Pause);
                    self.state.status = PlaybackStatus::Paused;
                    output.events.push(PlayerEvent::PlaybackPaused);
                }
            }
            PlayerCommand::Stop => {
                output.backend_actions.push(BackendAction::Stop);
                self.state.status = PlaybackStatus::Stopped;
                self.state.position_ms = 0;
                self.state.is_buffering = false;
                output.events.push(PlayerEvent::PlaybackStopped);
            }
            PlayerCommand::Seek { position_ms } => {
                let position = self
                    .state
                    .duration_ms
                    .map_or(position_ms, |duration| position_ms.min(duration));
                self.state.position_ms = position;
                output.backend_actions.push(BackendAction::SeekTo(position));
            }
            PlayerCommand::Next => self.advance(false, &mut output),
            PlayerCommand::Previous => {
                if self.state.position_ms > 3_000 {
                    self.state.position_ms = 0;
                    output.backend_actions.push(BackendAction::SeekTo(0));
                } else if let Some(track) = self.queue.previous().cloned() {
                    self.sync_selected_track(&track, &mut output, true);
                    output.backend_actions.push(BackendAction::Play);
                    self.state.status = PlaybackStatus::Playing;
                }
            }
            PlayerCommand::PlayTrack(id) => {
                let track = self.queue.select(&id)?.clone();
                self.sync_selected_track(&track, &mut output, true);
                output.backend_actions.push(BackendAction::Play);
                self.state.status = PlaybackStatus::Playing;
                output.events.push(PlayerEvent::PlaybackStarted);
            }
            PlayerCommand::SetQueue {
                tracks,
                start_index,
            } => {
                self.queue.replace(tracks, start_index)?;
                if let Some(selected) = self.queue.current().cloned() {
                    self.backend_loaded_track = None;
                    self.sync_selected_track(&selected, &mut output, false);
                }
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::AddToQueue(track) => {
                let had_current = self.queue.current().is_some();
                self.queue.enqueue(track);
                if !had_current {
                    if let Some(selected) = self.queue.current().cloned() {
                        self.sync_selected_track(&selected, &mut output, false);
                    }
                }
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::PlayNext(track) => {
                let had_current = self.queue.current().is_some();
                self.queue.enqueue_next(track);
                if !had_current {
                    if let Some(selected) = self.queue.current().cloned() {
                        self.sync_selected_track(&selected, &mut output, false);
                    }
                }
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::RemoveFromQueue(id) => {
                let previous_track = self.state.current_track.clone();
                if self.queue.remove(&id) {
                    if let Some(selected) = self.queue.current().cloned() {
                        if previous_track.as_ref() != Some(&selected.id) {
                            self.sync_selected_track(&selected, &mut output, true);
                            if self.state.status == PlaybackStatus::Playing {
                                output.backend_actions.push(BackendAction::Play);
                            }
                        }
                    } else {
                        self.backend_loaded_track = None;
                        self.state.current_track = None;
                        self.state.duration_ms = None;
                        self.state.position_ms = 0;
                        self.state.status = PlaybackStatus::Idle;
                        output.backend_actions.push(BackendAction::Stop);
                    }
                    output.events.push(PlayerEvent::QueueChanged);
                    output
                        .events
                        .push(PlayerEvent::TrackChanged(self.state.current_track.clone()));
                }
            }
            PlayerCommand::SetVolume(volume) => {
                if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
                    return Err(PlayerError::InvalidVolume);
                }
                self.state.volume = volume;
                output
                    .backend_actions
                    .push(BackendAction::SetVolume(volume));
            }
            PlayerCommand::SetSpeed(speed) => {
                if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
                    return Err(PlayerError::InvalidSpeed);
                }
                self.state.speed = speed;
                output.backend_actions.push(BackendAction::SetSpeed(speed));
            }
            PlayerCommand::SetRepeatMode(mode) => self.state.repeat_mode = mode,
            PlayerCommand::SetShuffle(enabled) => {
                self.queue.set_shuffle(enabled);
                self.state.shuffle_enabled = enabled;
            }
        }

        output
            .events
            .push(PlayerEvent::StateChanged(self.state.clone()));
        Ok(output)
    }

    /// Apply asynchronous callbacks from the native backend. Track IDs reject stale callbacks.
    pub fn handle_backend_event(&mut self, event: BackendEvent) -> CoreOutput {
        let mut output = CoreOutput::default();
        match event {
            BackendEvent::Prepared {
                track_id,
                duration_ms,
            } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.duration_ms = duration_ms;
                    self.state.position_ms = 0;
                    self.state.is_buffering = false;
                    if self.state.status == PlaybackStatus::Loading {
                        self.state.status = PlaybackStatus::Ready;
                    }
                }
            }
            BackendEvent::Position {
                track_id,
                position_ms,
            } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.position_ms = self
                        .state
                        .duration_ms
                        .map_or(position_ms, |duration| position_ms.min(duration));
                }
            }
            BackendEvent::Buffering {
                track_id,
                is_buffering,
            } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.is_buffering = is_buffering;
                    if is_buffering && self.state.status == PlaybackStatus::Playing {
                        self.state.status = PlaybackStatus::Loading;
                    } else if !is_buffering && self.state.status == PlaybackStatus::Loading {
                        self.state.status = PlaybackStatus::Playing;
                    }
                }
            }
            BackendEvent::Ended { track_id } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.advance(true, &mut output);
                }
            }
            BackendEvent::Failed {
                track_id,
                code,
                message,
            } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.status = PlaybackStatus::Error;
                    self.state.is_buffering = false;
                    output
                        .events
                        .push(PlayerEvent::PlaybackError { code, message });
                }
            }
        }
        output
            .events
            .push(PlayerEvent::StateChanged(self.state.clone()));
        output
    }

    fn advance(&mut self, from_completion: bool, output: &mut CoreOutput) {
        if self.state.repeat_mode == RepeatMode::One && from_completion {
            self.state.position_ms = 0;
            output.backend_actions.push(BackendAction::SeekTo(0));
            output.backend_actions.push(BackendAction::Play);
            self.state.status = PlaybackStatus::Playing;
            return;
        }
        if let Some(track) = self.queue.next(self.state.repeat_mode).cloned() {
            self.sync_selected_track(&track, output, true);
            output.backend_actions.push(BackendAction::Play);
            self.state.status = PlaybackStatus::Playing;
            output.events.push(PlayerEvent::PlaybackStarted);
        } else {
            self.state.status = PlaybackStatus::Completed;
            self.state.is_buffering = false;
            output.backend_actions.push(BackendAction::Stop);
            output.events.push(PlayerEvent::PlaybackCompleted);
        }
    }

    /// Legacy callback; prefer handle_backend_event to reject stale callbacks.
    /// Native backend callback after a media item has been prepared.
    pub fn on_prepared(&mut self, duration_ms: Option<u64>) -> PlayerEvent {
        self.state.duration_ms = duration_ms;
        self.state.position_ms = 0;
        if self.state.status != PlaybackStatus::Playing {
            self.state.status = PlaybackStatus::Ready;
        }
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Native backend callback for periodic playback position updates.
    pub fn on_position(&mut self, position_ms: u64) -> PlayerEvent {
        self.state.position_ms = self
            .state
            .duration_ms
            .map_or(position_ms, |duration| position_ms.min(duration));
        PlayerEvent::StateChanged(self.state.clone())
    }

    pub fn on_buffering(&mut self, buffering: bool) -> PlayerEvent {
        self.state.is_buffering = buffering;
        PlayerEvent::StateChanged(self.state.clone())
    }

    pub fn on_error(&mut self) -> PlayerEvent {
        self.state.status = PlaybackStatus::Error;
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    fn sync_selected_track(&mut self, track: &Track, output: &mut CoreOutput, prepare: bool) {
        let changed = self.state.current_track.as_ref() != Some(&track.id);
        let needs_prepare = prepare && self.backend_loaded_track.as_ref() != Some(&track.id);
        self.state.current_track = Some(track.id.clone());
        self.state.duration_ms = track.duration_ms;
        self.state.position_ms = 0;

        if needs_prepare {
            output
                .backend_actions
                .push(BackendAction::Prepare(track.clone()));
            self.backend_loaded_track = Some(track.id.clone());
        }
        if changed {
            output
                .events
                .push(PlayerEvent::TrackChanged(Some(track.id.clone())));
        }
    }

    /// Capture the complete logical player state for durable storage.
    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            version: PLAYER_SNAPSHOT_VERSION,
            state: self.state.clone(),
            queue: self.queue.snapshot(),
        }
    }

    /// Rebuild the logical player and emit the backend actions needed to re-prepare it.
    ///
    /// Playback is restored paused after process death. A native host must explicitly
    /// request Resume after it has re-established its audio session and user intent.
    pub fn restore(snapshot: PlayerSnapshot) -> Result<(Self, CoreOutput), SnapshotError> {
        if snapshot.version != PLAYER_SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion(snapshot.version));
        }

        let queue = PlaybackQueue::from_snapshot(snapshot.queue)?;
        if snapshot.state.current_track.as_ref() != queue.current().map(|track| &track.id)
            || snapshot.state.shuffle_enabled != queue.shuffle_enabled()
        {
            return Err(SnapshotError::CurrentTrackMismatch);
        }
        if !snapshot.state.volume.is_finite()
            || !(0.0..=1.0).contains(&snapshot.state.volume)
            || !snapshot.state.speed.is_finite()
            || !(0.25..=4.0).contains(&snapshot.state.speed)
        {
            return Err(SnapshotError::InvalidPlaybackSettings);
        }

        let mut state = snapshot.state;
        if matches!(
            state.status,
            PlaybackStatus::Playing | PlaybackStatus::Loading
        ) {
            state.status = PlaybackStatus::Paused;
        }
        state.is_buffering = false;
        if let Some(duration) = state.duration_ms {
            state.position_ms = state.position_ms.min(duration);
        }

        let mut output = CoreOutput::default();
        if let Some(track) = queue.current() {
            output
                .backend_actions
                .push(BackendAction::Prepare(track.clone()));
            output
                .backend_actions
                .push(BackendAction::SetVolume(state.volume));
            output
                .backend_actions
                .push(BackendAction::SetSpeed(state.speed));
            output
                .backend_actions
                .push(BackendAction::SeekTo(state.position_ms));
        }

        output.events.push(PlayerEvent::StateChanged(state.clone()));
        Ok((
            Self {
                state,
                queue,
                backend_loaded_track: None,
            },
            output,
        ))
    }

    pub fn current_track_id(&self) -> Option<&TrackId> {
        self.state.current_track.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;

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
    fn play_emits_prepare_and_play_for_first_track() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 12_000)))
            .unwrap();
        let output = core.handle(PlayerCommand::Play).unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert!(output
            .backend_actions
            .iter()
            .any(|action| matches!(action, BackendAction::Prepare(_))));
        assert!(output.backend_actions.contains(&BackendAction::Play));
        assert!(output
            .events
            .iter()
            .any(|event| matches!(event, PlayerEvent::PlaybackStarted)));
    }

    #[test]
    fn volume_and_speed_are_validated() {
        let mut core = PlayerCore::new();
        assert_eq!(
            core.handle(PlayerCommand::SetVolume(1.5)),
            Err(PlayerError::InvalidVolume)
        );
        assert_eq!(
            core.handle(PlayerCommand::SetSpeed(0.0)),
            Err(PlayerError::InvalidSpeed)
        );
    }

    #[test]
    fn seek_is_clamped_and_forwarded_to_backend() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 5_000)))
            .unwrap();
        core.handle(PlayerCommand::PlayTrack(TrackId::from("one")))
            .unwrap();
        core.on_prepared(Some(5_000));
        let output = core
            .handle(PlayerCommand::Seek { position_ms: 9_000 })
            .unwrap();
        assert_eq!(core.state().position_ms, 5_000);
        assert!(output
            .backend_actions
            .contains(&BackendAction::SeekTo(5_000)));
    }

    #[test]
    fn stale_backend_callbacks_are_ignored() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::AddToQueue(track("two", 2_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        core.handle(PlayerCommand::Next).unwrap();

        core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from("one"),
            position_ms: 700,
        });
        assert_eq!(core.state().position_ms, 0);
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
    }

    #[test]
    fn ended_event_advances_and_prepares_next_track() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::AddToQueue(track("two", 2_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        let output = core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert!(output.backend_actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("two"))
        ));
        assert!(output.backend_actions.contains(&BackendAction::Play));
    }

    #[test]
    fn backend_failure_updates_state_and_emits_error() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        let output = core.handle_backend_event(BackendEvent::Failed {
            track_id: TrackId::from("one"),
            code: "decoder_error".into(),
            message: "unsupported stream".into(),
        });
        assert_eq!(core.state().status, PlaybackStatus::Error);
        assert!(output.events.iter().any(
            |event| matches!(event, PlayerEvent::PlaybackError { code, .. } if code == "decoder_error")
        ));
    }

    #[test]
    fn next_prepares_the_next_track() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::AddToQueue(track("two", 2_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let output = core.handle(PlayerCommand::Next).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert!(output.backend_actions.iter().any(|action| matches!(action, BackendAction::Prepare(track) if track.id == TrackId::from("two"))));
    }

    #[test]
    fn shuffle_command_updates_queue_and_playback_state() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::SetQueue {
            tracks: vec![
                track("one", 1_000),
                track("two", 2_000),
                track("three", 3_000),
                track("four", 4_000),
            ],
            start_index: 0,
        })
        .unwrap();

        core.handle(PlayerCommand::SetShuffle(true)).unwrap();
        assert!(core.state().shuffle_enabled);
        assert!(core.queue().shuffle_enabled());
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));

        core.handle(PlayerCommand::SetShuffle(false)).unwrap();
        assert!(!core.state().shuffle_enabled);
        assert!(!core.queue().shuffle_enabled());
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
    }
    #[test]
    fn snapshot_round_trip_preserves_queue_and_restores_playback_paused() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::SetQueue {
            tracks: vec![
                track("one", 10_000),
                track("two", 20_000),
                track("three", 30_000),
            ],
            start_index: 0,
        })
        .unwrap();
        core.handle(PlayerCommand::SetShuffle(true)).unwrap();
        core.handle(PlayerCommand::SetVolume(0.65)).unwrap();
        core.handle(PlayerCommand::SetSpeed(1.25)).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from("one"),
            position_ms: 4_500,
        });

        let encoded = serde_json::to_string(&core.snapshot()).unwrap();
        let snapshot = serde_json::from_str(&encoded).unwrap();
        let (restored, output) = PlayerCore::restore(snapshot).unwrap();

        assert_eq!(restored.state().status, PlaybackStatus::Paused);
        assert_eq!(restored.state().position_ms, 4_500);
        assert_eq!(restored.state().volume, 0.65);
        assert_eq!(restored.state().speed, 1.25);
        assert!(restored.state().shuffle_enabled);
        assert_eq!(restored.current_track_id(), Some(&TrackId::from("one")));
        assert!(output.backend_actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("one"))
        ));
        assert!(output
            .backend_actions
            .contains(&BackendAction::SeekTo(4_500)));

        let next_after_restore = restored.queue().clone();
        let mut next_after_restore = next_after_restore;
        let expected_next = next_after_restore.next(RepeatMode::Off).unwrap().id.clone();
        let mut original_queue = core.queue().clone();
        assert_eq!(
            original_queue.next(RepeatMode::Off).unwrap().id,
            expected_next
        );
    }

    #[test]
    fn snapshot_rejects_unsupported_versions() {
        let core = PlayerCore::new();
        let mut snapshot = core.snapshot();
        snapshot.version += 1;
        assert!(matches!(
            PlayerCore::restore(snapshot),
            Err(SnapshotError::UnsupportedVersion(_))
        ));
    }
}
