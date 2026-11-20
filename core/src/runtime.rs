use crate::audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
use crate::backend::{dispatch_output, AudioBackend, BackendDispatchError};
use crate::engine::PlayerCore;
use crate::model::{
    BackendAction, BackendEvent, CoreOutput, PlayerCommand, PlayerError, PlayerEvent,
};
use crate::persistence::{PlayerSnapshot, SnapshotError};
use thiserror::Error;

/// Drives the platform-independent player core against a native audio backend.
///
/// When a backend action fails, the runtime marks the current track as failed and returns
/// an error. The events produced by that operation (including the resulting
/// `PlaybackError`) are kept and can be collected with `take_pending_events`.
pub struct PlayerRuntime<B> {
    core: PlayerCore,
    backend: B,
    audio_session: AudioSessionPolicy,
    pending_events: Vec<PlayerEvent>,
}

impl<B: AudioBackend> PlayerRuntime<B> {
    /// Restore a saved player snapshot and rehydrate the native backend.
    pub fn restore(
        backend: B,
        snapshot: PlayerSnapshot,
    ) -> Result<(Self, CoreOutput), RuntimeError> {
        let (core, output) = PlayerCore::restore(snapshot)?;
        let mut runtime = Self::from_parts(core, backend);
        runtime.dispatch_or_mark_failed(&output)?;
        Ok((runtime, output))
    }

    pub fn new(backend: B) -> Self {
        Self::from_parts(PlayerCore::new(), backend)
    }

    /// Create a runtime with a custom audio-focus policy (for example a different duck level).
    pub fn with_audio_session(backend: B, audio_session: AudioSessionPolicy) -> Self {
        let mut runtime = Self::new(backend);
        runtime.audio_session = audio_session;
        runtime
    }

    fn from_parts(core: PlayerCore, backend: B) -> Self {
        Self {
            core,
            backend,
            audio_session: AudioSessionPolicy::default(),
            pending_events: Vec::new(),
        }
    }

    pub fn core(&self) -> &PlayerCore {
        &self.core
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn audio_session(&self) -> &AudioSessionPolicy {
        &self.audio_session
    }

    /// Capture the logical player state for durable storage.
    pub fn snapshot(&self) -> PlayerSnapshot {
        self.core.snapshot()
    }

    /// Events produced by operations that ended in a backend failure.
    pub fn take_pending_events(&mut self) -> Vec<PlayerEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Apply a command to the core and immediately forward its backend actions.
    pub fn handle_command(&mut self, command: PlayerCommand) -> Result<CoreOutput, RuntimeError> {
        let output = self.core.handle(command)?;
        self.dispatch_or_mark_failed(&output)?;
        Ok(output)
    }

    /// Drain native callbacks, update core state, and dispatch any resulting actions.
    ///
    /// Every drained callback is processed even if forwarding an earlier one failed; the
    /// first failure is returned and the events are then available from `take_pending_events`.
    pub fn poll_backend(&mut self) -> Result<CoreOutput, RuntimeError> {
        let events = self.backend.poll_events();
        let mut combined = CoreOutput::default();
        let mut first_error = None;

        for event in events {
            let output = self.core.handle_backend_event(event);
            if let Err(error) = self.dispatch_or_mark_failed(&output) {
                first_error.get_or_insert(error);
                continue;
            }
            combined.backend_actions.extend(output.backend_actions);
            combined.events.extend(output.events);
        }

        match first_error {
            None => Ok(combined),
            Some(error) => {
                let mut kept = combined.events;
                kept.append(&mut self.pending_events);
                self.pending_events = kept;
                Err(error)
            }
        }
    }

    /// Translate an OS audio-focus change into pause/resume/duck operations on the player.
    ///
    /// Ducking changes only the native volume; the user's volume stays in the core state.
    /// Returns the combined output of everything that was applied.
    pub fn handle_audio_focus(
        &mut self,
        change: AudioFocusChange,
    ) -> Result<CoreOutput, RuntimeError> {
        let state = self.core.state();
        let actions = self
            .audio_session
            .handle(change, state.status, state.volume);
        let mut combined = CoreOutput::default();

        for action in actions {
            match action {
                AudioSessionAction::PausePlayback => {
                    let output = self.handle_command(PlayerCommand::Pause)?;
                    combined.backend_actions.extend(output.backend_actions);
                    combined.events.extend(output.events);
                }
                AudioSessionAction::ResumePlayback => {
                    let output = self.handle_command(PlayerCommand::Resume)?;
                    combined.backend_actions.extend(output.backend_actions);
                    combined.events.extend(output.events);
                }
                AudioSessionAction::SetEffectiveVolume(volume) => {
                    let output = CoreOutput {
                        backend_actions: vec![BackendAction::SetVolume(volume)],
                        events: Vec::new(),
                    };
                    // Already the effective (ducked or restored) volume: send as-is.
                    dispatch_output(&mut self.backend, &output)?;
                    combined.backend_actions.extend(output.backend_actions);
                }
            }
        }
        Ok(combined)
    }

    /// While ducked, user volume changes must stay scaled by the duck factor.
    fn apply_ducking(&self, output: &CoreOutput) -> CoreOutput {
        if !self.audio_session.is_ducked() {
            return output.clone();
        }
        let factor = self.audio_session.duck_factor();
        CoreOutput {
            backend_actions: output
                .backend_actions
                .iter()
                .cloned()
                .map(|action| match action {
                    BackendAction::SetVolume(volume) => BackendAction::SetVolume(volume * factor),
                    other => other,
                })
                .collect(),
            events: Vec::new(),
        }
    }

    fn dispatch_or_mark_failed(&mut self, output: &CoreOutput) -> Result<(), RuntimeError> {
        let adjusted = self.apply_ducking(output);
        if let Err(error) = dispatch_output(&mut self.backend, &adjusted) {
            self.pending_events.extend(output.events.iter().cloned());
            if let Some(track_id) = self.core.current_track_id().cloned() {
                let failure = BackendEvent::Failed {
                    track_id,
                    code: error.source.code.clone(),
                    message: error.source.message.clone(),
                };
                let failure_output = self.core.handle_backend_event(failure);
                self.pending_events.extend(failure_output.events);
            }
            return Err(RuntimeError::BackendDispatch(error));
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum RuntimeError {
    #[error(transparent)]
    Player(#[from] PlayerError),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    BackendDispatch(#[from] BackendDispatchError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AudioBackend, BackendError};
    use crate::model::{AudioSource, BackendAction, Track, TrackId};

    #[derive(Default)]
    struct FakeBackend {
        actions: Vec<BackendAction>,
        events: Vec<BackendEvent>,
        fail_next: bool,
    }

    impl AudioBackend for FakeBackend {
        fn execute(&mut self, action: BackendAction) -> Result<(), BackendError> {
            if self.fail_next {
                self.fail_next = false;
                return Err(BackendError::new("fake_failure", "backend rejected action"));
            }
            self.actions.push(action);
            Ok(())
        }

        fn poll_events(&mut self) -> Vec<BackendEvent> {
            std::mem::take(&mut self.events)
        }
    }

    fn track() -> Track {
        Track {
            id: TrackId::from("runtime-track"),
            title: "Runtime track".into(),
            artist: None,
            album: None,
            duration_ms: Some(4_000),
            source: AudioSource::LocalFile("/music/runtime.flac".into()),
        }
    }

    #[test]
    fn command_actions_are_forwarded_to_backend() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        runtime.handle_command(PlayerCommand::Play).unwrap();

        assert!(runtime
            .backend()
            .actions
            .iter()
            .any(|action| matches!(action, BackendAction::Prepare(_))));
        assert!(runtime.backend().actions.contains(&BackendAction::Play));
    }

    #[test]
    fn polled_end_event_advances_and_dispatches_next_track() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        let mut next = track();
        next.id = TrackId::from("next-track");
        runtime
            .handle_command(PlayerCommand::AddToQueue(next.clone()))
            .unwrap();
        runtime.handle_command(PlayerCommand::Play).unwrap();

        runtime.backend_mut().events.push(BackendEvent::Ended {
            track_id: TrackId::from("runtime-track"),
        });
        let output = runtime.poll_backend().unwrap();

        assert_eq!(
            runtime.core().current_track_id(),
            Some(&TrackId::from("next-track"))
        );
        assert!(output
            .events
            .iter()
            .any(|event| matches!(event, PlayerEvent::TrackChanged(Some(id)) if id == &next.id)));
        assert!(runtime
            .backend()
            .actions
            .iter()
            .any(|action| matches!(action, BackendAction::Prepare(item) if item.id == next.id)));
    }

    #[test]
    fn backend_dispatch_failure_marks_current_track_as_error() {
        let backend = FakeBackend {
            fail_next: false,
            ..FakeBackend::default()
        };
        let mut runtime = PlayerRuntime::new(backend);
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        runtime.backend_mut().fail_next = true;

        let error = runtime.handle_command(PlayerCommand::Play).unwrap_err();

        assert!(matches!(error, RuntimeError::BackendDispatch(_)));
        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Error
        );
    }
    #[test]
    fn retry_after_dispatch_failure_reprepares_current_track() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        runtime.backend_mut().fail_next = true;

        assert!(matches!(
            runtime.handle_command(PlayerCommand::Play),
            Err(RuntimeError::BackendDispatch(_))
        ));
        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Error
        );

        let retry = runtime.handle_command(PlayerCommand::Play).unwrap();
        assert!(retry.backend_actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("runtime-track"))
        ));
        assert!(runtime.backend().actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("runtime-track"))
        ));
    }

    #[test]
    fn runtime_restore_rehydrates_backend_without_autoplay() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track())).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let snapshot = core.snapshot();

        let (runtime, output) = PlayerRuntime::restore(FakeBackend::default(), snapshot).unwrap();

        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Paused
        );
        assert!(runtime.backend().actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("runtime-track"))
        ));
        assert!(runtime
            .backend()
            .actions
            .contains(&BackendAction::SeekTo(0)));
        assert!(!runtime.backend().actions.contains(&BackendAction::Play));
        assert!(!output.backend_actions.contains(&BackendAction::Play));
    }

    #[test]
    fn audio_focus_loss_pauses_and_gain_resumes_through_the_backend() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        runtime.handle_command(PlayerCommand::Play).unwrap();

        runtime
            .handle_audio_focus(AudioFocusChange::TransientLoss)
            .unwrap();
        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Paused
        );
        assert_eq!(
            runtime.backend().actions.last(),
            Some(&BackendAction::Pause)
        );

        runtime
            .handle_audio_focus(AudioFocusChange::Gain {
                should_resume: true,
            })
            .unwrap();
        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Playing
        );
        assert_eq!(runtime.backend().actions.last(), Some(&BackendAction::Play));
    }

    #[test]
    fn ducking_scales_native_volume_without_changing_user_volume() {
        let mut runtime =
            PlayerRuntime::with_audio_session(FakeBackend::default(), AudioSessionPolicy::new(0.5));
        runtime
            .handle_command(PlayerCommand::SetVolume(0.8))
            .unwrap();
        runtime.handle_audio_focus(AudioFocusChange::Duck).unwrap();
        assert_eq!(
            runtime.backend().actions.last(),
            Some(&BackendAction::SetVolume(0.4))
        );
        assert_eq!(runtime.core().state().volume, 0.8);

        // A user volume change while ducked stays ducked.
        runtime
            .handle_command(PlayerCommand::SetVolume(0.6))
            .unwrap();
        assert_eq!(
            runtime.backend().actions.last(),
            Some(&BackendAction::SetVolume(0.3))
        );

        runtime
            .handle_audio_focus(AudioFocusChange::Gain {
                should_resume: false,
            })
            .unwrap();
        assert_eq!(
            runtime.backend().actions.last(),
            Some(&BackendAction::SetVolume(0.6))
        );
    }

    #[test]
    fn failure_events_survive_a_backend_dispatch_error() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        runtime.backend_mut().fail_next = true;
        assert!(runtime.handle_command(PlayerCommand::Play).is_err());

        let events = runtime.take_pending_events();
        assert!(events
            .iter()
            .any(|event| matches!(event, PlayerEvent::PlaybackError { code, .. } if code == "fake_failure")));
        assert!(runtime.take_pending_events().is_empty());
    }

    #[test]
    fn runtime_snapshot_round_trips_through_restore() {
        let mut runtime = PlayerRuntime::new(FakeBackend::default());
        runtime
            .handle_command(PlayerCommand::AddToQueue(track()))
            .unwrap();
        let (restored, _) =
            PlayerRuntime::restore(FakeBackend::default(), runtime.snapshot()).unwrap();
        assert_eq!(
            restored.core().current_track_id(),
            Some(&TrackId::from("runtime-track"))
        );
    }
}
