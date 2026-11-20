use crate::backend::{dispatch_output, AudioBackend, BackendDispatchError};
use crate::engine::PlayerCore;
use crate::model::{BackendEvent, CoreOutput, PlayerCommand, PlayerError, PlayerEvent};
use crate::persistence::{PlayerSnapshot, SnapshotError};
use thiserror::Error;

/// Drives the platform-independent player core against a native audio backend.
pub struct PlayerRuntime<B> {
    core: PlayerCore,
    backend: B,
}

impl<B: AudioBackend> PlayerRuntime<B> {
    /// Restore a saved player snapshot and rehydrate the native backend.
    pub fn restore(
        backend: B,
        snapshot: PlayerSnapshot,
    ) -> Result<(Self, CoreOutput), RuntimeError> {
        let (core, output) = PlayerCore::restore(snapshot)?;
        let mut runtime = Self { core, backend };
        runtime.dispatch_or_mark_failed(&output)?;
        Ok((runtime, output))
    }

    pub fn new(backend: B) -> Self {
        Self {
            core: PlayerCore::new(),
            backend,
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

    /// Apply a command to the core and immediately forward its backend actions.
    pub fn handle_command(&mut self, command: PlayerCommand) -> Result<CoreOutput, RuntimeError> {
        let output = self.core.handle(command)?;
        self.dispatch_or_mark_failed(&output)?;
        Ok(output)
    }

    /// Drain native callbacks, update core state, and dispatch any resulting actions.
    pub fn poll_backend(&mut self) -> Result<CoreOutput, RuntimeError> {
        let events = self.backend.poll_events();
        let mut combined = CoreOutput::default();

        for event in events {
            let output = self.core.handle_backend_event(event);
            self.dispatch_or_mark_failed(&output)?;
            combined.backend_actions.extend(output.backend_actions);
            combined.events.extend(output.events);
        }

        Ok(combined)
    }

    fn dispatch_or_mark_failed(&mut self, output: &CoreOutput) -> Result<(), RuntimeError> {
        if let Err(error) = dispatch_output(&mut self.backend, output) {
            if let Some(track_id) = self.core.current_track_id().cloned() {
                let failure = BackendEvent::Failed {
                    track_id,
                    code: error.source.code.clone(),
                    message: error.source.message.clone(),
                };
                let failure_output = self.core.handle_backend_event(failure);
                debug_assert!(failure_output
                    .events
                    .iter()
                    .any(|event| matches!(event, PlayerEvent::PlaybackError { .. })));
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
    fn runtime_restore_rehydrates_backend_without_autoplay() {
        let mut core = PlayerCore::new();
        core.handle(PlayerCommand::AddToQueue(track())).unwrap();
        core.handle(PlayerCommand::Play).unwrap();
        let snapshot = core.snapshot();

        let (runtime, output) =
            PlayerRuntime::restore(FakeBackend::default(), snapshot).unwrap();

        assert_eq!(
            runtime.core().state().status,
            crate::model::PlaybackStatus::Paused
        );
        assert!(runtime.backend().actions.iter().any(
            |action| matches!(action, BackendAction::Prepare(item) if item.id == TrackId::from("runtime-track"))
        ));
        assert!(runtime.backend().actions.contains(&BackendAction::SeekTo(0)));
        assert!(!runtime.backend().actions.contains(&BackendAction::Play));
        assert!(!output.backend_actions.contains(&BackendAction::Play));
    }

}
