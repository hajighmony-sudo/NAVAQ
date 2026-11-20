use crate::model::{BackendAction, BackendEvent, CoreOutput};
use thiserror::Error;

/// A normalized failure returned by a native audio implementation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct BackendError {
    pub code: String,
    pub message: String,
}

impl BackendError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Contract implemented by the Android Media3 and iOS AVFoundation adapters.
///
/// Implementations should enqueue platform work and report its asynchronous
/// results through `poll_events`; they must not call back into PlayerCore
/// while an action is being executed.
pub trait AudioBackend {
    fn execute(&mut self, action: BackendAction) -> Result<(), BackendError>;

    /// Return all currently available native callbacks without blocking.
    fn poll_events(&mut self) -> Vec<BackendEvent>;
}

/// Identifies the action that failed while forwarding a core output.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("backend action at index {action_index} failed: {source}")]
pub struct BackendDispatchError {
    pub action_index: usize,
    #[source]
    pub source: BackendError,
}

/// Forward a core's ordered backend actions to a platform implementation.
///
/// Actions are executed in order. The first failure is returned immediately;
/// later actions are not sent because their preconditions may no longer hold.
pub fn dispatch_output<B: AudioBackend>(
    backend: &mut B,
    output: &CoreOutput,
) -> Result<(), BackendDispatchError> {
    for (action_index, action) in output.backend_actions.iter().cloned().enumerate() {
        backend
            .execute(action)
            .map_err(|source| BackendDispatchError {
                action_index,
                source,
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioSource, Track, TrackId};

    #[derive(Default)]
    struct RecordingBackend {
        actions: Vec<BackendAction>,
        fail_at: Option<usize>,
    }

    impl AudioBackend for RecordingBackend {
        fn execute(&mut self, action: BackendAction) -> Result<(), BackendError> {
            if self.fail_at == Some(self.actions.len()) {
                return Err(BackendError::new("platform_failure", "test failure"));
            }
            self.actions.push(action);
            Ok(())
        }

        fn poll_events(&mut self) -> Vec<BackendEvent> {
            Vec::new()
        }
    }

    fn sample_track() -> Track {
        Track {
            id: TrackId::from("track-1"),
            title: "Track 1".into(),
            artist: None,
            album: None,
            duration_ms: None,
            source: AudioSource::LocalFile("/music/track-1.flac".into()),
        }
    }

    #[test]
    fn dispatches_actions_in_the_original_order() {
        let mut backend = RecordingBackend::default();
        let output = CoreOutput {
            backend_actions: vec![
                BackendAction::Prepare(sample_track()),
                BackendAction::SetVolume(0.7),
                BackendAction::Play,
            ],
            events: Vec::new(),
        };

        dispatch_output(&mut backend, &output).unwrap();

        assert_eq!(backend.actions, output.backend_actions);
    }

    #[test]
    fn stops_dispatching_after_the_first_backend_failure() {
        let mut backend = RecordingBackend {
            actions: Vec::new(),
            fail_at: Some(1),
        };
        let output = CoreOutput {
            backend_actions: vec![
                BackendAction::Prepare(sample_track()),
                BackendAction::Play,
                BackendAction::Pause,
            ],
            events: Vec::new(),
        };

        let error = dispatch_output(&mut backend, &output).unwrap_err();

        assert_eq!(error.action_index, 1);
        assert_eq!(error.source.code, "platform_failure");
        assert_eq!(backend.actions, vec![BackendAction::Prepare(sample_track())]);
    }
}
