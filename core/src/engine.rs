use crate::model::{
    BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus, PlayerCommand,
    PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
use crate::persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
use crate::queue::PlaybackQueue;

/// Longest supported crossfade.
pub const MAX_CROSSFADE_MS: u32 = 12_000;

/// Previous restarts the current track instead of going back once it has played this long.
const PREVIOUS_RESTART_THRESHOLD_MS: u64 = 3_000;

/// Platform-independent playback coordinator.
///
/// The core owns queue and playback state. It emits explicit backend actions; native
/// Android/iOS adapters execute those actions using Media3 or AVFoundation respectively.
#[derive(Debug, Default)]
pub struct PlayerCore {
    state: PlaybackState,
    queue: PlaybackQueue,
    /// The track the native backend currently has prepared (and therefore positioned).
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

    /// The track that follows the current one on natural completion, for gapless preloading.
    pub fn upcoming_track(&self) -> Option<&Track> {
        self.queue.peek_next(self.state.repeat_mode)
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
                self.play_selected(&track, &mut output);
            }
            PlayerCommand::Pause => {
                if self.state.status == PlaybackStatus::Playing {
                    output.backend_actions.push(BackendAction::Pause);
                    self.state.status = PlaybackStatus::Paused;
                    output.events.push(PlayerEvent::PlaybackPaused);
                }
            }
            PlayerCommand::Stop => {
                if self.state.current_track.is_some() {
                    output.backend_actions.push(BackendAction::Stop);
                    // Stop releases the native item; the next Play must prepare it again.
                    self.backend_loaded_track = None;
                    self.state.status = PlaybackStatus::Stopped;
                    self.state.position_ms = 0;
                    self.state.is_buffering = false;
                    output.events.push(PlayerEvent::PlaybackStopped);
                }
            }
            PlayerCommand::Seek { position_ms } => {
                if self.state.current_track.is_none() {
                    return Err(PlayerError::EmptyQueue);
                }
                let position = self
                    .state
                    .duration_ms
                    .map_or(position_ms, |duration| position_ms.min(duration));
                self.state.position_ms = position;
                output.backend_actions.push(BackendAction::SeekTo(position));
            }
            PlayerCommand::Next => {
                if self.queue.is_empty() {
                    return Err(PlayerError::EmptyQueue);
                }
                self.advance(false, &mut output);
            }
            PlayerCommand::Previous => {
                if self.state.position_ms > PREVIOUS_RESTART_THRESHOLD_MS {
                    self.state.position_ms = 0;
                    output.backend_actions.push(BackendAction::SeekTo(0));
                } else if let Some(track) = self
                    .queue
                    .previous_with_repeat(self.state.repeat_mode)
                    .cloned()
                {
                    self.begin_track(&track, &mut output);
                }
            }
            PlayerCommand::PlayTrack(id) => {
                let track = self.queue.select(&id)?.clone();
                self.play_selected(&track, &mut output);
            }
            PlayerCommand::SetQueue {
                tracks,
                start_index,
            } => {
                let was_playing = matches!(
                    self.state.status,
                    PlaybackStatus::Playing | PlaybackStatus::Loading
                );
                self.queue.replace(tracks, start_index)?;
                if let Some(selected) = self.queue.current().cloned() {
                    self.backend_loaded_track = None;
                    if was_playing {
                        // Replacing an active queue must not leave the old native
                        // item audible while the logical current track has changed.
                        output.backend_actions.push(BackendAction::Stop);
                        self.state.status = PlaybackStatus::Paused;
                    } else if matches!(
                        self.state.status,
                        PlaybackStatus::Completed | PlaybackStatus::Stopped | PlaybackStatus::Error
                    ) {
                        self.state.status = PlaybackStatus::Idle;
                    }
                    self.state.is_buffering = false;
                    self.sync_selected_track(&selected, &mut output, false);
                } else {
                    self.reset_to_empty(&mut output);
                }
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::AddToQueue(track) => {
                let had_current = self.queue.current().is_some();
                self.queue.enqueue(track)?;
                if !had_current {
                    if let Some(selected) = self.queue.current().cloned() {
                        self.sync_selected_track(&selected, &mut output, false);
                    }
                }
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::PlayNext(track) => {
                let had_current = self.queue.current().is_some();
                self.queue.enqueue_next(track)?;
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
                    match self.queue.current().cloned() {
                        Some(selected) => {
                            if previous_track.as_ref() != Some(&selected.id) {
                                let playing = self.state.status == PlaybackStatus::Playing;
                                self.sync_selected_track(&selected, &mut output, true);
                                if playing {
                                    output.backend_actions.push(BackendAction::Play);
                                }
                            }
                        }
                        None => self.reset_to_empty(&mut output),
                    }
                    output.events.push(PlayerEvent::QueueChanged);
                }
            }
            PlayerCommand::MoveInQueue {
                track_id,
                new_index,
            } => {
                self.queue.move_track(&track_id, new_index)?;
                output.events.push(PlayerEvent::QueueChanged);
            }
            PlayerCommand::ClearQueue => {
                if !self.queue.is_empty() {
                    self.queue.reset();
                    self.reset_to_empty(&mut output);
                    output.events.push(PlayerEvent::QueueChanged);
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
            PlayerCommand::SetEqualizer(equalizer) => {
                if !equalizer.is_valid() {
                    return Err(PlayerError::InvalidEqualizer);
                }
                self.state.equalizer = equalizer.clone();
                output
                    .backend_actions
                    .push(BackendAction::SetEqualizer(equalizer));
            }
            PlayerCommand::SetCrossfade(crossfade_ms) => {
                if crossfade_ms > MAX_CROSSFADE_MS {
                    return Err(PlayerError::InvalidCrossfade);
                }
                self.state.crossfade_ms = crossfade_ms;
                output
                    .backend_actions
                    .push(BackendAction::SetCrossfade(crossfade_ms));
            }
            PlayerCommand::SetRepeatMode(mode) => self.state.repeat_mode = mode,
            PlayerCommand::SetConsume(enabled) => self.state.consume = enabled,
            PlayerCommand::SetSingle(enabled) => self.state.single = enabled,
            PlayerCommand::SetLoopRegion(region) => {
                if let Some(region) = region {
                    if self.state.current_track.is_none() {
                        return Err(PlayerError::EmptyQueue);
                    }
                    let fits = self
                        .state
                        .duration_ms
                        .map_or(true, |duration| region.end_ms <= duration);
                    if region.start_ms >= region.end_ms || !fits {
                        return Err(PlayerError::InvalidLoopRegion);
                    }
                }
                self.state.loop_region = region;
            }
            PlayerCommand::Reshuffle { seed } => {
                if self.queue.reshuffle(seed) {
                    output.events.push(PlayerEvent::QueueChanged);
                }
            }
            PlayerCommand::SetShuffle(enabled) => {
                let changed = self.queue.shuffle_enabled() != enabled;
                self.queue.set_shuffle(enabled);
                self.state.shuffle_enabled = enabled;
                if changed {
                    output.events.push(PlayerEvent::QueueChanged);
                }
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
                    // Keep the position: a restored track is seeked right after Prepare.
                    self.state.duration_ms = duration_ms.or(self.state.duration_ms);
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
                // A late tick after Stop/completion/error must not move a finished track.
                let accepts_position = matches!(
                    self.state.status,
                    PlaybackStatus::Loading
                        | PlaybackStatus::Ready
                        | PlaybackStatus::Playing
                        | PlaybackStatus::Paused
                );
                if accepts_position && self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.position_ms = self
                        .state
                        .duration_ms
                        .map_or(position_ms, |duration| position_ms.min(duration));
                    // A-B loop: jump back as soon as the end marker is reached.
                    if let Some(region) = self.state.loop_region {
                        if self.state.status == PlaybackStatus::Playing
                            && self.state.position_ms >= region.end_ms
                        {
                            self.state.position_ms = region.start_ms;
                            output
                                .backend_actions
                                .push(BackendAction::SeekTo(region.start_ms));
                        }
                    }
                }
            }
            BackendEvent::Buffering {
                track_id,
                is_buffering,
            } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    // Buffering is orthogonal to the user's playback intent. Keep
                    // Playing so Pause remains available during network stalls.
                    self.state.is_buffering = is_buffering;
                }
            }
            BackendEvent::Ended { track_id } => {
                // An end callback that races with Pause/Stop must not restart playback.
                if self.state.current_track.as_ref() == Some(&track_id)
                    && self.state.status == PlaybackStatus::Playing
                {
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
                    // A failed prepare/command does not mean the native backend has
                    // a usable copy of this track. Force a fresh Prepare on retry.
                    self.backend_loaded_track = None;
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

    /// Move to the next track (or finish). `from_completion` is true when the current
    /// track ended naturally, which is the only case where repeat-one applies.
    fn advance(&mut self, from_completion: bool, output: &mut CoreOutput) {
        if self.state.repeat_mode == RepeatMode::One && from_completion {
            self.state.position_ms = 0;
            output.backend_actions.push(BackendAction::SeekTo(0));
            output.backend_actions.push(BackendAction::Play);
            self.state.status = PlaybackStatus::Playing;
            return;
        }
        if from_completion && self.state.single {
            self.finish(output);
            return;
        }
        let previous = self.state.current_track.clone();
        if let Some(track) = self.queue.next(self.state.repeat_mode).cloned() {
            self.begin_track(&track, output);
            if self.state.consume && previous.as_ref() != Some(&track.id) {
                if let Some(previous) = previous {
                    if self.queue.remove(&previous) {
                        output.events.push(PlayerEvent::QueueChanged);
                    }
                }
            }
        } else {
            self.finish(output);
        }
    }

    /// Playback ran out of tracks (or single mode ended the track).
    fn finish(&mut self, output: &mut CoreOutput) {
        self.state.status = PlaybackStatus::Completed;
        self.state.is_buffering = false;
        self.backend_loaded_track = None;
        output.backend_actions.push(BackendAction::Stop);
        output.events.push(PlayerEvent::PlaybackCompleted);
    }

    /// Select `track` and play it from the beginning, even if it is already loaded
    /// (for example Previous at the first track, or repeat-all over a single track).
    fn begin_track(&mut self, track: &Track, output: &mut CoreOutput) {
        let already_loaded = self.state.current_track.as_ref() == Some(&track.id)
            && self.backend_loaded_track.as_ref() == Some(&track.id);
        self.sync_selected_track(track, output, true);
        if already_loaded {
            self.state.position_ms = 0;
            output.backend_actions.push(BackendAction::SeekTo(0));
        }
        output.backend_actions.push(BackendAction::Play);
        self.state.status = PlaybackStatus::Playing;
        output.events.push(PlayerEvent::PlaybackStarted);
    }

    /// Start or continue playing an already-selected track without losing its position.
    fn play_selected(&mut self, track: &Track, output: &mut CoreOutput) {
        if matches!(
            self.state.status,
            PlaybackStatus::Completed | PlaybackStatus::Stopped
        ) {
            // The native item was released; start over from a fresh Prepare.
            self.backend_loaded_track = None;
        }
        let already_playing = self.state.status == PlaybackStatus::Playing
            && self.state.current_track.as_ref() == Some(&track.id)
            && self.backend_loaded_track.as_ref() == Some(&track.id);
        if already_playing {
            return;
        }
        self.sync_selected_track(track, output, true);
        output.backend_actions.push(BackendAction::Play);
        self.state.status = PlaybackStatus::Playing;
        output.events.push(PlayerEvent::PlaybackStarted);
    }

    fn reset_to_empty(&mut self, output: &mut CoreOutput) {
        self.backend_loaded_track = None;
        self.state.current_track = None;
        self.state.duration_ms = None;
        self.state.position_ms = 0;
        self.state.status = PlaybackStatus::Idle;
        self.state.is_buffering = false;
        output.backend_actions.push(BackendAction::Stop);
        output.events.push(PlayerEvent::TrackChanged(None));
    }

    /// Legacy callback; prefer `handle_backend_event`, which rejects stale callbacks.
    #[deprecated(note = "use handle_backend_event(BackendEvent::Prepared { .. })")]
    pub fn on_prepared(&mut self, duration_ms: Option<u64>) -> PlayerEvent {
        self.state.duration_ms = duration_ms;
        self.state.position_ms = 0;
        if self.state.status != PlaybackStatus::Playing {
            self.state.status = PlaybackStatus::Ready;
        }
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Legacy callback; prefer `handle_backend_event`, which rejects stale callbacks.
    #[deprecated(note = "use handle_backend_event(BackendEvent::Position { .. })")]
    pub fn on_position(&mut self, position_ms: u64) -> PlayerEvent {
        self.state.position_ms = self
            .state
            .duration_ms
            .map_or(position_ms, |duration| position_ms.min(duration));
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Legacy callback; prefer `handle_backend_event`, which rejects stale callbacks.
    #[deprecated(note = "use handle_backend_event(BackendEvent::Buffering { .. })")]
    pub fn on_buffering(&mut self, buffering: bool) -> PlayerEvent {
        self.state.is_buffering = buffering;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Legacy callback; prefer `handle_backend_event`, which rejects stale callbacks.
    #[deprecated(note = "use handle_backend_event(BackendEvent::Failed { .. })")]
    pub fn on_error(&mut self) -> PlayerEvent {
        self.state.status = PlaybackStatus::Error;
        self.state.is_buffering = false;
        PlayerEvent::StateChanged(self.state.clone())
    }

    /// Make `track` the logical current track. Position and duration restart only when
    /// the selection changes or the backend does not hold this track yet, so Resume and
    /// restored sessions keep their position.
    fn sync_selected_track(&mut self, track: &Track, output: &mut CoreOutput, prepare: bool) {
        let changed = self.state.current_track.as_ref() != Some(&track.id);
        let loaded = self.backend_loaded_track.as_ref() == Some(&track.id);
        if changed {
            self.state.loop_region = None;
        }
        if changed || !loaded {
            self.state.duration_ms = track.duration_ms;
            self.state.position_ms = 0;
            self.state.is_buffering = false;
        }
        self.state.current_track = Some(track.id.clone());

        if prepare && !loaded {
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
            || !snapshot.state.equalizer.is_valid()
            || snapshot.state.crossfade_ms > MAX_CROSSFADE_MS
            || snapshot
                .state
                .loop_region
                .is_some_and(|region| region.start_ms >= region.end_ms)
        {
            return Err(SnapshotError::InvalidPlaybackSettings);
        }

        let mut state = snapshot.state;
        if matches!(
            state.status,
            PlaybackStatus::Playing | PlaybackStatus::Loading | PlaybackStatus::Error
        ) {
            state.status = PlaybackStatus::Paused;
        }
        state.is_buffering = false;
        if let Some(duration) = state.duration_ms {
            state.position_ms = state.position_ms.min(duration);
        }

        let mut output = CoreOutput::default();
        let mut backend_loaded_track = None;
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
                .push(BackendAction::SetEqualizer(state.equalizer.clone()));
            output
                .backend_actions
                .push(BackendAction::SetCrossfade(state.crossfade_ms));
            output
                .backend_actions
                .push(BackendAction::SeekTo(state.position_ms));
            // The restore actions already prepared and positioned this track; a later
            // Resume must continue from here instead of preparing it again.
            backend_loaded_track = Some(track.id.clone());
        }

        output.events.push(PlayerEvent::StateChanged(state.clone()));
        Ok((
            Self {
                state,
                queue,
                backend_loaded_track,
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
    fn replacing_queue_during_playback_stops_old_track_and_pauses() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("old", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        let output = core
            .handle(PlayerCommand::SetQueue {
                tracks: vec![track("new", 2_000)],
                start_index: 0,
            })
            .unwrap();

        assert_eq!(core.current_track_id(), Some(&TrackId::from("new")));
        assert_eq!(core.state().status, PlaybackStatus::Paused);
        assert!(output.backend_actions.contains(&BackendAction::Stop));
    }

    #[test]
    fn setting_an_empty_queue_stops_and_resets_the_player() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        let output = core
            .handle(PlayerCommand::SetQueue {
                tracks: Vec::new(),
                start_index: 0,
            })
            .unwrap();

        assert!(core.queue().is_empty());
        assert_eq!(core.state().status, PlaybackStatus::Idle);
        assert_eq!(core.current_track_id(), None);
        assert_eq!(core.state().position_ms, 0);
        assert!(output.backend_actions.contains(&BackendAction::Stop));
        assert!(output.events.contains(&PlayerEvent::TrackChanged(None)));
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
        core.handle_backend_event(BackendEvent::Prepared {
            track_id: TrackId::from("one"),
            duration_ms: Some(5_000),
        });
        let output = core
            .handle(PlayerCommand::Seek { position_ms: 9_000 })
            .unwrap();
        assert_eq!(core.state().position_ms, 5_000);
        assert!(output
            .backend_actions
            .contains(&BackendAction::SeekTo(5_000)));
    }

    #[test]
    fn buffering_does_not_hide_playing_intent_or_block_pause() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 5_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        core.handle_backend_event(BackendEvent::Buffering {
            track_id: TrackId::from("one"),
            is_buffering: true,
        });
        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert!(core.state().is_buffering);

        let pause = core.handle(PlayerCommand::Pause).unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Paused);
        assert!(pause.backend_actions.contains(&BackendAction::Pause));
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
    fn retry_after_backend_failure_prepares_track_again() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track("one", 1_000)))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();

        core.handle_backend_event(BackendEvent::Failed {
            track_id: TrackId::from("one"),
            code: "temporary_failure".into(),
            message: "temporary backend failure".into(),
        });

        let retry = core.handle(PlayerCommand::Play).unwrap();
        assert!(retry.backend_actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("one"))
        ));
        assert!(retry.backend_actions.contains(&BackendAction::Play));
        assert_eq!(core.state().status, PlaybackStatus::Playing);
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

    fn position(core: &mut PlayerCore, id: &str, position_ms: u64) {
        core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from(id),
            position_ms,
        });
    }

    fn core_with(ids: &[&str]) -> PlayerCore {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::SetQueue {
            tracks: ids.iter().map(|id| track(id, 60_000)).collect(),
            start_index: 0,
        })
        .unwrap();
        core
    }

    #[test]
    fn resume_keeps_position_and_does_not_prepare_again() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 30_000);
        core.handle(PlayerCommand::Pause).unwrap();

        let output = core.handle(PlayerCommand::Resume).unwrap();

        assert_eq!(core.state().position_ms, 30_000);
        assert_eq!(output.backend_actions, vec![BackendAction::Play]);
    }

    #[test]
    fn play_while_already_playing_is_a_no_op() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        let output = core.handle(PlayerCommand::Play).unwrap();
        assert!(output.backend_actions.is_empty());
        assert!(!output.events.contains(&PlayerEvent::PlaybackStarted));
    }

    #[test]
    fn resume_after_restore_continues_without_reprepare() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 12_000);
        let (mut restored, _) = PlayerCore::restore(core.snapshot()).unwrap();

        let output = restored.handle(PlayerCommand::Resume).unwrap();

        assert_eq!(restored.state().position_ms, 12_000);
        assert_eq!(output.backend_actions, vec![BackendAction::Play]);
    }

    #[test]
    fn prepared_callback_does_not_reset_restored_position() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 9_000);
        let (mut restored, _) = PlayerCore::restore(core.snapshot()).unwrap();
        restored.handle_backend_event(BackendEvent::Prepared {
            track_id: TrackId::from("one"),
            duration_ms: Some(60_000),
        });
        assert_eq!(restored.state().position_ms, 9_000);
    }

    #[test]
    fn previous_at_the_first_track_restarts_it_on_the_backend() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 1_000);

        let output = core.handle(PlayerCommand::Previous).unwrap();

        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
        assert_eq!(core.state().position_ms, 0);
        assert_eq!(
            output.backend_actions,
            vec![BackendAction::SeekTo(0), BackendAction::Play]
        );
    }

    #[test]
    fn previous_after_three_seconds_only_seeks_to_start() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Next).unwrap();
        position(&mut core, "two", 10_000);
        let output = core.handle(PlayerCommand::Previous).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert_eq!(output.backend_actions, vec![BackendAction::SeekTo(0)]);
    }

    #[test]
    fn previous_wraps_to_last_track_with_repeat_all() {
        let mut core = core_with(&["one", "two", "three"]);
        core.handle(PlayerCommand::SetRepeatMode(RepeatMode::All))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        core.handle(PlayerCommand::Previous).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("three")));
    }

    #[test]
    fn repeat_all_over_a_single_track_restarts_it() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::SetRepeatMode(RepeatMode::All))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 59_000);

        let output = core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });

        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert_eq!(core.state().position_ms, 0);
        assert!(output.backend_actions.contains(&BackendAction::SeekTo(0)));
        assert!(output.backend_actions.contains(&BackendAction::Play));
    }

    #[test]
    fn repeat_one_restarts_on_completion_but_next_still_advances() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::SetRepeatMode(RepeatMode::One))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
        core.handle(PlayerCommand::Next).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
    }

    #[test]
    fn stop_then_play_prepares_again_from_the_start() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 5_000);
        core.handle(PlayerCommand::Stop).unwrap();
        assert_eq!(core.state().position_ms, 0);

        let output = core.handle(PlayerCommand::Play).unwrap();

        assert!(matches!(
            output.backend_actions.first(),
            Some(BackendAction::Prepare(item)) if item.id == TrackId::from("one")
        ));
        assert_eq!(core.state().position_ms, 0);
    }

    #[test]
    fn play_after_the_queue_completed_restarts_the_last_track() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.state().status, PlaybackStatus::Completed);

        let output = core.handle(PlayerCommand::Play).unwrap();

        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert!(output
            .backend_actions
            .iter()
            .any(|action| matches!(action, BackendAction::Prepare(_))));
    }

    #[test]
    fn late_position_after_stop_and_early_ended_after_pause_are_ignored() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle(PlayerCommand::Pause).unwrap();
        let output = core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert!(output.backend_actions.is_empty());
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));

        core.handle(PlayerCommand::Stop).unwrap();
        position(&mut core, "one", 4_000);
        assert_eq!(core.state().position_ms, 0);
    }

    #[test]
    fn next_after_an_error_skips_to_the_next_track() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Failed {
            track_id: TrackId::from("one"),
            code: "decode".into(),
            message: "bad file".into(),
        });
        core.handle(PlayerCommand::Next).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert_eq!(core.state().status, PlaybackStatus::Playing);
    }

    #[test]
    fn seek_and_next_without_tracks_report_empty_queue() {
        let mut core = PlayerCore::new();
        assert_eq!(
            core.handle(PlayerCommand::Seek { position_ms: 5 }),
            Err(PlayerError::EmptyQueue)
        );
        assert_eq!(
            core.handle(PlayerCommand::Next),
            Err(PlayerError::EmptyQueue)
        );
    }

    #[test]
    fn duplicate_tracks_are_rejected_and_leave_state_untouched() {
        let mut core = core_with(&["one"]);
        assert_eq!(
            core.handle(PlayerCommand::AddToQueue(track("one", 1))),
            Err(PlayerError::DuplicateTrack)
        );
        assert_eq!(
            core.handle(PlayerCommand::PlayNext(track("one", 1))),
            Err(PlayerError::DuplicateTrack)
        );
        assert_eq!(
            core.handle(PlayerCommand::SetQueue {
                tracks: vec![track("a", 1), track("a", 1)],
                start_index: 0
            }),
            Err(PlayerError::DuplicateTrack)
        );
        assert_eq!(core.queue().len(), 1);
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
    }

    #[test]
    fn removing_the_playing_track_plays_the_replacement_once() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();

        let output = core
            .handle(PlayerCommand::RemoveFromQueue(TrackId::from("one")))
            .unwrap();

        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert!(output.backend_actions.contains(&BackendAction::Play));
        let changes = output
            .events
            .iter()
            .filter(|event| matches!(event, PlayerEvent::TrackChanged(_)))
            .count();
        assert_eq!(changes, 1);
    }

    #[test]
    fn removing_a_non_current_track_keeps_playback_untouched() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        position(&mut core, "one", 7_000);
        let output = core
            .handle(PlayerCommand::RemoveFromQueue(TrackId::from("two")))
            .unwrap();
        assert!(output.backend_actions.is_empty());
        assert_eq!(core.state().position_ms, 7_000);
        assert!(!output
            .events
            .iter()
            .any(|event| matches!(event, PlayerEvent::TrackChanged(_))));
    }

    #[test]
    fn removing_the_last_track_resets_to_idle() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        let output = core
            .handle(PlayerCommand::RemoveFromQueue(TrackId::from("one")))
            .unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Idle);
        assert!(output.backend_actions.contains(&BackendAction::Stop));
        assert!(output.events.contains(&PlayerEvent::TrackChanged(None)));
    }

    #[test]
    fn clear_queue_stops_and_resets_the_player() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::Play).unwrap();
        let output = core.handle(PlayerCommand::ClearQueue).unwrap();
        assert!(core.queue().is_empty());
        assert_eq!(core.state().status, PlaybackStatus::Idle);
        assert_eq!(core.current_track_id(), None);
        assert!(output.backend_actions.contains(&BackendAction::Stop));
        assert!(output.events.contains(&PlayerEvent::QueueChanged));
        // Clearing an empty queue is a harmless no-op.
        let again = core.handle(PlayerCommand::ClearQueue).unwrap();
        assert!(again.backend_actions.is_empty());
    }

    #[test]
    fn move_in_queue_changes_what_plays_next() {
        let mut core = core_with(&["one", "two", "three"]);
        core.handle(PlayerCommand::MoveInQueue {
            track_id: TrackId::from("three"),
            new_index: 1,
        })
        .unwrap();
        assert_eq!(core.upcoming_track().unwrap().id, TrackId::from("three"));
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
        assert_eq!(
            core.handle(PlayerCommand::MoveInQueue {
                track_id: TrackId::from("three"),
                new_index: 7
            }),
            Err(PlayerError::InvalidQueueIndex)
        );
    }

    #[test]
    fn upcoming_track_follows_repeat_mode() {
        let mut core = core_with(&["one", "two"]);
        assert_eq!(core.upcoming_track().unwrap().id, TrackId::from("two"));
        core.handle(PlayerCommand::Next).unwrap();
        assert!(core.upcoming_track().is_none());
        core.handle(PlayerCommand::SetRepeatMode(RepeatMode::All))
            .unwrap();
        assert_eq!(core.upcoming_track().unwrap().id, TrackId::from("one"));
    }

    #[test]
    fn replacing_the_queue_after_completion_returns_to_idle() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        core.handle(PlayerCommand::SetQueue {
            tracks: vec![track("one", 60_000)],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Idle);
        assert_eq!(core.state().position_ms, 0);
        let play = core.handle(PlayerCommand::Play).unwrap();
        assert!(matches!(
            play.backend_actions.first(),
            Some(BackendAction::Prepare(_))
        ));
    }

    #[test]
    fn restoring_an_errored_session_comes_back_paused() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Failed {
            track_id: TrackId::from("one"),
            code: "x".into(),
            message: "y".into(),
        });
        let (restored, _) = PlayerCore::restore(core.snapshot()).unwrap();
        assert_eq!(restored.state().status, PlaybackStatus::Paused);
    }

    #[test]
    fn equalizer_and_crossfade_are_validated_stored_and_restored() {
        use crate::equalizer::{EqualizerPreset, EqualizerSettings};
        let mut core = core_with(&["one"]);
        let rock = EqualizerSettings::preset(EqualizerPreset::Rock);
        let output = core
            .handle(PlayerCommand::SetEqualizer(rock.clone()))
            .unwrap();
        assert_eq!(
            output.backend_actions,
            vec![BackendAction::SetEqualizer(rock.clone())]
        );
        core.handle(PlayerCommand::SetCrossfade(3_000)).unwrap();

        let mut bad = rock.clone();
        bad.bands[0].gain_db = 40.0;
        assert_eq!(
            core.handle(PlayerCommand::SetEqualizer(bad)),
            Err(PlayerError::InvalidEqualizer)
        );
        assert_eq!(
            core.handle(PlayerCommand::SetCrossfade(99_999)),
            Err(PlayerError::InvalidCrossfade)
        );

        let (restored, output) = PlayerCore::restore(core.snapshot()).unwrap();
        assert_eq!(restored.state().equalizer, rock);
        assert_eq!(restored.state().crossfade_ms, 3_000);
        assert!(output
            .backend_actions
            .contains(&BackendAction::SetEqualizer(rock)));
        assert!(output
            .backend_actions
            .contains(&BackendAction::SetCrossfade(3_000)));
    }

    #[test]
    fn snapshot_with_invalid_equalizer_is_rejected() {
        let mut snapshot = core_with(&["one"]).snapshot();
        snapshot.state.equalizer.bands.clear();
        assert!(matches!(
            PlayerCore::restore(snapshot),
            Err(SnapshotError::InvalidPlaybackSettings)
        ));
    }

    use crate::model::LoopRegion;

    #[test]
    fn single_mode_stops_after_the_track_but_manual_next_still_works() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::SetSingle(true)).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let output = core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.state().status, PlaybackStatus::Completed);
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
        assert!(output.events.contains(&PlayerEvent::PlaybackCompleted));

        core.handle(PlayerCommand::Next).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
    }

    #[test]
    fn single_with_repeat_one_keeps_repeating() {
        let mut core = core_with(&["one", "two"]);
        core.handle(PlayerCommand::SetSingle(true)).unwrap();
        core.handle(PlayerCommand::SetRepeatMode(RepeatMode::One))
            .unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.state().status, PlaybackStatus::Playing);
        assert_eq!(core.current_track_id(), Some(&TrackId::from("one")));
    }

    #[test]
    fn consume_removes_played_tracks_and_keeps_the_current_one() {
        let mut core = core_with(&["one", "two", "three"]);
        core.handle(PlayerCommand::SetConsume(true)).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let output = core.handle_backend_event(BackendEvent::Ended {
            track_id: TrackId::from("one"),
        });
        assert_eq!(core.current_track_id(), Some(&TrackId::from("two")));
        assert_eq!(core.queue().len(), 2);
        assert!(output.events.contains(&PlayerEvent::QueueChanged));
        core.handle(PlayerCommand::Next).unwrap();
        assert_eq!(core.current_track_id(), Some(&TrackId::from("three")));
        assert_eq!(core.queue().len(), 1);
        assert_eq!(core.upcoming_track(), None);
    }

    #[test]
    fn consume_works_with_shuffle() {
        let mut core = core_with(&["a", "b", "c", "d", "e"]);
        core.handle(PlayerCommand::SetShuffle(true)).unwrap();
        core.handle(PlayerCommand::SetConsume(true)).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let mut seen = vec![core.current_track_id().unwrap().0.clone()];
        for _ in 0..4 {
            core.handle(PlayerCommand::Next).unwrap();
            seen.push(core.current_track_id().unwrap().0.clone());
        }
        seen.sort();
        assert_eq!(seen, ["a", "b", "c", "d", "e"]);
        assert_eq!(core.queue().len(), 1);
    }

    #[test]
    fn loop_region_seeks_back_when_the_end_is_reached() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::Play).unwrap();
        core.handle(PlayerCommand::SetLoopRegion(Some(LoopRegion {
            start_ms: 10_000,
            end_ms: 20_000,
        })))
        .unwrap();
        let early = core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from("one"),
            position_ms: 19_000,
        });
        assert!(early.backend_actions.is_empty());
        let wrap = core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from("one"),
            position_ms: 20_100,
        });
        assert_eq!(wrap.backend_actions, vec![BackendAction::SeekTo(10_000)]);
        assert_eq!(core.state().position_ms, 10_000);

        core.handle(PlayerCommand::SetLoopRegion(None)).unwrap();
        let free = core.handle_backend_event(BackendEvent::Position {
            track_id: TrackId::from("one"),
            position_ms: 30_000,
        });
        assert!(free.backend_actions.is_empty());
    }

    #[test]
    fn loop_region_is_validated_and_cleared_on_track_change() {
        let mut core = core_with(&["one", "two"]);
        let bad =
            |start_ms, end_ms| PlayerCommand::SetLoopRegion(Some(LoopRegion { start_ms, end_ms }));
        assert_eq!(core.handle(bad(5, 5)), Err(PlayerError::InvalidLoopRegion));
        assert_eq!(
            core.handle(bad(0, 999_999)),
            Err(PlayerError::InvalidLoopRegion)
        );
        core.handle(bad(1_000, 2_000)).unwrap();
        assert!(core.state().loop_region.is_some());
        core.handle(PlayerCommand::Next).unwrap();
        assert!(core.state().loop_region.is_none());

        let mut empty = PlayerCore::new();
        assert_eq!(empty.handle(bad(0, 10)), Err(PlayerError::EmptyQueue));
    }

    #[test]
    fn reshuffle_command_only_acts_with_shuffle_enabled() {
        let mut core = core_with(&["a", "b", "c", "d", "e", "f", "g"]);
        let off = core.handle(PlayerCommand::Reshuffle { seed: 7 }).unwrap();
        assert!(!off.events.contains(&PlayerEvent::QueueChanged));
        core.handle(PlayerCommand::SetShuffle(true)).unwrap();
        let on = core.handle(PlayerCommand::Reshuffle { seed: 7 }).unwrap();
        assert!(on.events.contains(&PlayerEvent::QueueChanged));
        assert_eq!(core.current_track_id(), Some(&TrackId::from("a")));
    }

    #[test]
    fn snapshot_keeps_modes_and_rejects_a_bad_loop_region() {
        let mut core = core_with(&["one"]);
        core.handle(PlayerCommand::SetConsume(true)).unwrap();
        core.handle(PlayerCommand::SetSingle(true)).unwrap();
        let (restored, _) = PlayerCore::restore(core.snapshot()).unwrap();
        assert!(restored.state().consume && restored.state().single);

        let mut snapshot = core.snapshot();
        snapshot.state.loop_region = Some(LoopRegion {
            start_ms: 9,
            end_ms: 3,
        });
        assert!(matches!(
            PlayerCore::restore(snapshot),
            Err(SnapshotError::InvalidPlaybackSettings)
        ));
    }
}
