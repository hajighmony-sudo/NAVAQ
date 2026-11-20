//! Platform-independent audio focus and interruption policy.
//!
//! Native adapters translate Android audio-focus callbacks and iOS audio-session
//! interruptions into these events, then execute the returned actions.

use crate::model::PlaybackStatus;

/// Why the operating system changed this app's audio focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFocusChange {
    /// A short interruption (for example, a temporary system prompt).
    TransientLoss,
    /// Audio focus was taken away and is not expected to return automatically.
    PermanentLoss,
    /// Another app requests lower-priority audio without requiring a pause.
    Duck,
    /// Audio focus is available again.
    Gain { should_resume: bool },
}

/// Commands a native adapter should execute in response to an audio-focus change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioSessionAction {
    PausePlayback,
    ResumePlayback,
    SetEffectiveVolume(f32),
}

/// Stateful policy shared by Android and iOS adapters.
///
/// Ducking never overwrites the user's configured volume. The adapter applies
/// SetEffectiveVolume to the native player while retaining the core volume.
#[derive(Debug, Clone)]
pub struct AudioSessionPolicy {
    duck_factor: f32,
    was_playing_before_loss: bool,
    is_ducked: bool,
}

impl Default for AudioSessionPolicy {
    fn default() -> Self {
        Self::new(0.2)
    }
}

impl AudioSessionPolicy {
    /// Creates a policy with a duck multiplier between 0.0 and 1.0.
    /// Invalid values fall back to 0.2.
    pub fn new(duck_factor: f32) -> Self {
        let duck_factor = if duck_factor.is_finite() && (0.0..=1.0).contains(&duck_factor) {
            duck_factor
        } else {
            0.2
        };

        Self {
            duck_factor,
            was_playing_before_loss: false,
            is_ducked: false,
        }
    }

    /// Converts an OS audio-focus change into deterministic backend actions.
    pub fn handle(
        &mut self,
        change: AudioFocusChange,
        playback_status: PlaybackStatus,
        user_volume: f32,
    ) -> Vec<AudioSessionAction> {
        match change {
            AudioFocusChange::TransientLoss => {
                self.was_playing_before_loss |= playback_status == PlaybackStatus::Playing;
                let mut actions = self.restore_volume_if_ducked(user_volume);
                if playback_status == PlaybackStatus::Playing {
                    actions.push(AudioSessionAction::PausePlayback);
                }
                actions
            }
            AudioFocusChange::PermanentLoss => {
                self.was_playing_before_loss = false;
                let mut actions = self.restore_volume_if_ducked(user_volume);
                if playback_status == PlaybackStatus::Playing {
                    actions.push(AudioSessionAction::PausePlayback);
                }
                actions
            }
            AudioFocusChange::Duck => {
                if self.is_ducked {
                    Vec::new()
                } else {
                    self.is_ducked = true;
                    vec![AudioSessionAction::SetEffectiveVolume(
                        normalized_volume(user_volume) * self.duck_factor,
                    )]
                }
            }
            AudioFocusChange::Gain { should_resume } => {
                let mut actions = self.restore_volume_if_ducked(user_volume);
                if self.was_playing_before_loss {
                    self.was_playing_before_loss = false;
                    if should_resume {
                        actions.push(AudioSessionAction::ResumePlayback);
                    }
                }
                actions
            }
        }
    }

    fn restore_volume_if_ducked(&mut self, user_volume: f32) -> Vec<AudioSessionAction> {
        if self.is_ducked {
            self.is_ducked = false;
            vec![AudioSessionAction::SetEffectiveVolume(
                normalized_volume(user_volume),
            )]
        } else {
            Vec::new()
        }
    }

    pub fn is_ducked(&self) -> bool {
        self.is_ducked
    }
}

fn normalized_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_loss_pauses_only_active_playback_and_gain_can_resume() {
        let mut policy = AudioSessionPolicy::default();
        assert_eq!(
            policy.handle(
                AudioFocusChange::TransientLoss,
                PlaybackStatus::Playing,
                0.8
            ),
            vec![AudioSessionAction::PausePlayback]
        );
        assert_eq!(
            policy.handle(
                AudioFocusChange::Gain { should_resume: true },
                PlaybackStatus::Paused,
                0.8
            ),
            vec![AudioSessionAction::ResumePlayback]
        );
    }

    #[test]
    fn repeated_transient_loss_keeps_original_resume_intent() {
        let mut policy = AudioSessionPolicy::default();
        policy.handle(
            AudioFocusChange::TransientLoss,
            PlaybackStatus::Playing,
            0.5,
        );
        assert_eq!(
            policy.handle(
                AudioFocusChange::TransientLoss,
                PlaybackStatus::Paused,
                0.5
            ),
            Vec::<AudioSessionAction>::new()
        );
        assert_eq!(
            policy.handle(
                AudioFocusChange::Gain { should_resume: true },
                PlaybackStatus::Paused,
                0.5
            ),
            vec![AudioSessionAction::ResumePlayback]
        );
    }

    #[test]
    fn transient_loss_does_not_resume_when_os_denies_resume() {
        let mut policy = AudioSessionPolicy::default();
        policy.handle(
            AudioFocusChange::TransientLoss,
            PlaybackStatus::Playing,
            0.5,
        );
        assert!(policy
            .handle(
                AudioFocusChange::Gain {
                    should_resume: false
                },
                PlaybackStatus::Paused,
                0.5
            )
            .is_empty());
    }

    #[test]
    fn permanent_loss_never_resumes_automatically() {
        let mut policy = AudioSessionPolicy::default();
        assert_eq!(
            policy.handle(
                AudioFocusChange::PermanentLoss,
                PlaybackStatus::Playing,
                0.5
            ),
            vec![AudioSessionAction::PausePlayback]
        );
        assert!(policy
            .handle(
                AudioFocusChange::Gain { should_resume: true },
                PlaybackStatus::Paused,
                0.5
            )
            .is_empty());
    }

    #[test]
    fn ducking_preserves_user_volume_and_restores_it_on_gain() {
        let mut policy = AudioSessionPolicy::default();
        assert_eq!(
            policy.handle(AudioFocusChange::Duck, PlaybackStatus::Playing, 0.75),
            vec![AudioSessionAction::SetEffectiveVolume(0.15)]
        );
        assert!(policy
            .handle(AudioFocusChange::Duck, PlaybackStatus::Playing, 0.75)
            .is_empty());
        assert_eq!(
            policy.handle(
                AudioFocusChange::Gain { should_resume: false },
                PlaybackStatus::Playing,
                0.75
            ),
            vec![AudioSessionAction::SetEffectiveVolume(0.75)]
        );
        assert!(!policy.is_ducked());
    }

    #[test]
    fn interruption_while_ducked_restores_volume_before_pausing() {
        let mut policy = AudioSessionPolicy::default();
        policy.handle(AudioFocusChange::Duck, PlaybackStatus::Playing, 0.6);
        assert_eq!(
            policy.handle(
                AudioFocusChange::TransientLoss,
                PlaybackStatus::Playing,
                0.6
            ),
            vec![
                AudioSessionAction::SetEffectiveVolume(0.6),
                AudioSessionAction::PausePlayback
            ]
        );
        assert!(!policy.is_ducked());
    }

    #[test]
    fn invalid_duck_factor_uses_safe_default() {
        let mut policy = AudioSessionPolicy::new(f32::NAN);
        assert_eq!(
            policy.handle(AudioFocusChange::Duck, PlaybackStatus::Playing, 1.0),
            vec![AudioSessionAction::SetEffectiveVolume(0.2)]
        );
    }
}
