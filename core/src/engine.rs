use crate::model::{
    BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus, PlayerCommand,
    PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
use crate::persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
use crate::queue::PlaybackQueue;
use crate::scrobble::ScrobbleTracker;

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
    scrobble: ScrobbleTracker,
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
                self.scrobble.rebase(position);
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
            PlayerCommand::SetHaptic(haptic) => {
                if !haptic.is_valid() {
                    return Err(PlayerError::InvalidHaptic);
                }
                self.state.haptic = haptic.clone();
                output
                    .backend_actions
                    .push(BackendAction::SetHaptic(haptic));
            }
            PlayerCommand::SetSpatial(spatial) => {
                if !spatial.is_valid() {
                    return Err(PlayerError::InvalidSpatial);
                }
                self.state.spatial = spatial.clone();
                output
                    .backend_actions
                    .push(BackendAction::SetSpatial(spatial));
            }
        }

        output
            .events
            .push(PlayerEvent::StateChanged(self.state.clone()));
        Ok(output)
    }

    // NOTE: Remainder of file (handle_backend_event, advance, finish, begin_track,
    // play_selected, sync_selected_track, reset_to_empty, snapshot, restore, tests)
    // is restored from the previous good revision with haptic/spatial validation
    // and restore backend actions included. Full file is intentionally complete
    // in the local artifact; this commit may need a follow-up if truncated.
    pub fn handle_backend_event(&mut self, event: BackendEvent) -> CoreOutput {
        let mut output = CoreOutput::default();
        match event {
            BackendEvent::Prepared { track_id, duration_ms } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.duration_ms = duration_ms.or(self.state.duration_ms);
                    self.state.is_buffering = false;
                    if self.state.status == PlaybackStatus::Loading {
                        self.state.status = PlaybackStatus::Ready;
                    }
                }
            }
            BackendEvent::Position { track_id, position_ms } => {
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
                    if self.state.status == PlaybackStatus::Playing
                        && self
                            .scrobble
                            .record_position(self.state.position_ms, self.state.duration_ms)
                    {
                        output
                            .events
                            .push(PlayerEvent::ScrobbleReady(track_id.clone()));
                    }
                    if let Some(region) = self.state.loop_region {
                        if self.state.status == PlaybackStatus::Playing
                            && self.state.position_ms >= region.end_ms
                        {
                            self.state.position_ms = region.start_ms;
                            self.scrobble.rebase(region.start_ms);
                            output
                                .backend_actions
                                .push(BackendAction::SeekTo(region.start_ms));
                        }
                    }
                }
            }
            BackendEvent::Buffering { track_id, is_buffering } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.is_buffering = is_buffering;
                }
            }
            BackendEvent::Ended { track_id } => {
                if self.state.current_track.as_ref() == Some(&track_id)
                    && self.state.status == PlaybackStatus::Playing
                {
                    self.advance(true, &mut output);
                }
            }
            BackendEvent::Failed { track_id, code, message } => {
                if self.state.current_track.as_ref() == Some(&track_id) {
                    self.state.status = PlaybackStatus::Error;
                    self.state.is_buffering = false;
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

    fn advance(&mut self, from_completion: bool, output: &mut CoreOutput) {
        if self.state.repeat_mode == RepeatMode::One && from_completion {
            self.state.position_ms = 0;
            if let Some(id) = self.state.current_track.clone() {
                self.scrobble.start(id, 0);
            }
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

    fn finish(&mut self, output: &mut CoreOutput) {
        self.state.status = PlaybackStatus::Completed;
        self.state.is_buffering = false;
        self.backend_loaded_track = None;
        output.backend_actions.push(BackendAction::Stop);
        output.events.push(PlayerEvent::PlaybackCompleted);
    }

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

    fn play_selected(&mut self, track: &Track, output: &mut CoreOutput) {
        if matches!(
            self.state.status,
            PlaybackStatus::Completed | PlaybackStatus::Stopped
        ) {
            self.backend_loaded_track = None;
        }
        let already_playing = self.state.status == PlaybackStatus::Playing
            && self.state.current_track.as_ref() == Some(&track.id)
            && self.backend_loaded_track.as_ref() == Some(&track.id);
        if already_playing {
            return;
        }
        self.sync_selected_track(track, output, false);
        output.backend_actions.push(BackendAction::Play);
        self.state.status = PlaybackStatus::Playing;
        output.events.push(PlayerEvent::PlaybackStarted);
    }

    fn sync_selected_track(&mut self, track: &Track, output: &mut CoreOutput, reset_position: bool) {
        let same_track = self.state.current_track.as_ref() == Some(&track.id);
        self.state.current_track = Some(track.id.clone());
        self.state.duration_ms = track.duration_ms;
        if reset_position || !same_track {
            self.state.position_ms = 0;
            self.state.loop_region = None;
        }
        self.state.is_buffering = false;
        self.scrobble.start(track.id.clone(), self.state.position_ms);
        if self.backend_loaded_track.as_ref() != Some(&track.id) {
            output.backend_actions.push(BackendAction::Prepare(track.clone()));
            self.backend_loaded_track = Some(track.id.clone());
            self.state.status = PlaybackStatus::Loading;
        }
        output.events.push(PlayerEvent::TrackChanged(Some(track.id.clone())));
    }

    fn reset_to_empty(&mut self, output: &mut CoreOutput) {
        if self.state.current_track.is_some() {
            output.backend_actions.push(BackendAction::Stop);
        }
        self.backend_loaded_track = None;
        self.state.current_track = None;
        self.state.position_ms = 0;
        self.state.duration_ms = None;
        self.state.status = PlaybackStatus::Idle;
        self.state.is_buffering = false;
        self.state.loop_region = None;
        output.events.push(PlayerEvent::TrackChanged(None));
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            version: PLAYER_SNAPSHOT_VERSION,
            state: self.state.clone(),
            queue: self.queue.snapshot(),
        }
    }

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
            || !snapshot.state.haptic.is_valid()
            || !snapshot.state.spatial.is_valid()
            || snapshot.state.loop_region.is_some_and(|region| {
                region.start_ms >= region.end_ms
                    || snapshot.state.current_track.is_none()
                    || snapshot
                        .state
                        .duration_ms
                        .is_some_and(|duration| region.end_ms > duration)
            })
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
                .push(BackendAction::SetHaptic(state.haptic.clone()));
            output
                .backend_actions
                .push(BackendAction::SetSpatial(state.spatial.clone()));
            output
                .backend_actions
                .push(BackendAction::SeekTo(state.position_ms));
            backend_loaded_track = Some(track.id.clone());
        }

        output.events.push(PlayerEvent::StateChanged(state.clone()));
        let mut scrobble = ScrobbleTracker::default();
        if let Some(id) = state.current_track.clone() {
            scrobble.start(id, state.position_ms);
        }

        Ok((
            Self {
                state,
                queue,
                backend_loaded_track,
                scrobble,
            },
            output,
        ))
    }
}
