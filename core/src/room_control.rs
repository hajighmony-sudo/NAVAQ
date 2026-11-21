//! Simultaneous control of a shared listening room: permissioned commands from any
//! participant, a shared queue, versioning for ordering, and clock-drift correction.
//!
//! The host (or relay server) is the single authority: it applies commands in arrival
//! order with its own clock, so concurrent commands resolve as "last accepted wins" and
//! every accepted command bumps `SharedRoom::revision`.

use crate::model::Track;
use crate::social::{LiveEffect, ParticipantId, SharedRoom, SocialError};
use serde::{Deserialize, Serialize};

/// Who may control playback, the shared queue and room-wide effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    HostOnly,
    /// Everybody in the room controls together (the party default).
    #[default]
    Everyone,
    /// The host plus participants granted control.
    Delegated,
}

/// An action a participant asks the room to perform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlCommand {
    Play,
    Pause,
    Seek {
        position_ms: u64,
    },
    SetTrack {
        track: Option<Track>,
    },
    Enqueue {
        track: Track,
    },
    SkipToNext,
    AddGlobalEffect {
        effect: LiveEffect,
    },
    ClearGlobalEffects,
    /// Effects (echo, vocal overlay...) only for the sender; no control right needed.
    AddMyEffect {
        effect: LiveEffect,
    },
    ClearMyEffects,
    // Host only:
    SetControlMode {
        mode: ControlMode,
    },
    GrantControl {
        participant: ParticipantId,
    },
    RevokeControl {
        participant: ParticipantId,
    },
}

/// What a client should do to follow the room clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriftAction {
    InSync,
    /// Small drift: play slightly faster/slower (1.0 = normal) until caught up.
    AdjustSpeed(f32),
    /// Large drift: jump to this position.
    SeekTo(u64),
}

/// Drift below this is inaudible.
pub const DRIFT_TOLERANCE_MS: u64 = 80;
/// Drift above this is corrected with a seek instead of a speed nudge.
pub const DRIFT_SEEK_THRESHOLD_MS: u64 = 1_500;
const SPEED_NUDGE: f32 = 0.04;

impl SharedRoom {
    pub fn can_control(&self, participant: &ParticipantId) -> bool {
        if !self.participants.contains(participant) {
            return false;
        }
        self.is_host(participant)
            || match self.control_mode {
                ControlMode::HostOnly => false,
                ControlMode::Everyone => true,
                ControlMode::Delegated => self.controllers.contains(participant),
            }
    }

    /// Where playback should be right now on the room clock.
    pub fn current_position_ms(&self, now_ms: u64) -> u64 {
        let mut position = self.position_ms;
        if let (true, Some(anchor)) = (self.is_playing, self.anchor_at_ms) {
            position = position.saturating_add(now_ms.saturating_sub(anchor));
        }
        match self.current_track.as_ref().and_then(|t| t.duration_ms) {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    /// Compare a client's local position with the room clock.
    pub fn correct_drift(&self, local_position_ms: u64, now_ms: u64) -> DriftAction {
        let expected = self.current_position_ms(now_ms);
        let drift = expected.abs_diff(local_position_ms);
        if !self.is_playing || drift <= DRIFT_TOLERANCE_MS {
            if !self.is_playing && drift > DRIFT_TOLERANCE_MS {
                return DriftAction::SeekTo(expected);
            }
            return DriftAction::InSync;
        }
        if drift >= DRIFT_SEEK_THRESHOLD_MS {
            return DriftAction::SeekTo(expected);
        }
        // Behind the room -> speed up, ahead -> slow down.
        if local_position_ms < expected {
            DriftAction::AdjustSpeed(1.0 + SPEED_NUDGE)
        } else {
            DriftAction::AdjustSpeed(1.0 - SPEED_NUDGE)
        }
    }

    /// Apply a command from `from` at host time `now_ms`. Returns the new room version.
    pub fn apply_control(
        &mut self,
        from: &ParticipantId,
        command: ControlCommand,
        now_ms: u64,
    ) -> Result<u64, SocialError> {
        if !self.participants.contains(from) {
            return Err(SocialError::NotInRoom);
        }
        let controls = self.can_control(from);
        let host = self.is_host(from);

        match command {
            ControlCommand::AddMyEffect { effect } => {
                self.apply_participant_effect(from, effect)?;
            }
            ControlCommand::ClearMyEffects => {
                self.participant_effects.remove(from);
            }
            ControlCommand::SetControlMode { mode } => {
                require(host)?;
                self.control_mode = mode;
            }
            ControlCommand::GrantControl { participant } => {
                require(host)?;
                if !self.participants.contains(&participant) {
                    return Err(SocialError::NotInRoom);
                }
                self.controllers.insert(participant);
            }
            ControlCommand::RevokeControl { participant } => {
                require(host)?;
                self.controllers.remove(&participant);
            }
            other => {
                require(controls)?;
                self.apply_controlled(other, now_ms)?;
            }
        }
        self.revision = self.revision.saturating_add(1);
        Ok(self.revision)
    }

    fn apply_controlled(
        &mut self,
        command: ControlCommand,
        now_ms: u64,
    ) -> Result<(), SocialError> {
        match command {
            ControlCommand::Play => {
                self.position_ms = self.current_position_ms(now_ms);
                self.is_playing = self.current_track.is_some();
                self.anchor_at_ms = Some(now_ms);
            }
            ControlCommand::Pause => {
                self.position_ms = self.current_position_ms(now_ms);
                self.is_playing = false;
                self.anchor_at_ms = Some(now_ms);
            }
            ControlCommand::Seek { position_ms } => {
                self.position_ms = match self.current_track.as_ref().and_then(|t| t.duration_ms) {
                    Some(duration) => position_ms.min(duration),
                    None => position_ms,
                };
                self.anchor_at_ms = Some(now_ms);
            }
            ControlCommand::SetTrack { track } => {
                self.set_track(track);
                if self.current_track.is_none() {
                    self.is_playing = false;
                }
                self.anchor_at_ms = Some(now_ms);
            }
            ControlCommand::Enqueue { track } => {
                if self.current_track.is_none() {
                    self.set_track(Some(track));
                    self.anchor_at_ms = Some(now_ms);
                } else {
                    self.queue.push(track);
                }
            }
            ControlCommand::SkipToNext => self.skip_to_next(now_ms),
            ControlCommand::AddGlobalEffect { effect } => self.apply_global_effect(effect)?,
            ControlCommand::ClearGlobalEffects => self.clear_global_effects(),
            // Handled by the caller; kept exhaustive for safety.
            ControlCommand::AddMyEffect { .. }
            | ControlCommand::ClearMyEffects
            | ControlCommand::SetControlMode { .. }
            | ControlCommand::GrantControl { .. }
            | ControlCommand::RevokeControl { .. } => {}
        }
        Ok(())
    }

    fn skip_to_next(&mut self, now_ms: u64) {
        if self.queue.is_empty() {
            self.set_track(None);
            self.is_playing = false;
        } else {
            let next = self.queue.remove(0);
            self.set_track(Some(next));
        }
        self.anchor_at_ms = Some(now_ms);
    }

    /// Host tick: move to the next queued track when the current one has finished.
    /// Returns true if the room advanced (the host should broadcast the new state).
    pub fn advance_if_finished(&mut self, now_ms: u64) -> bool {
        let finished = self.is_playing
            && self
                .current_track
                .as_ref()
                .and_then(|t| t.duration_ms)
                .is_some_and(|duration| self.current_position_ms(now_ms) >= duration);
        if finished {
            self.skip_to_next(now_ms);
            self.revision = self.revision.saturating_add(1);
        }
        finished
    }
}

fn require(allowed: bool) -> Result<(), SocialError> {
    if allowed {
        Ok(())
    } else {
        Err(SocialError::PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;
    use crate::social::RoomId;

    fn track(id: &str, duration_ms: u64) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            album: None,
            duration_ms: Some(duration_ms),
            source: AudioSource::LocalFile(format!("/{id}.mp3")),
        }
    }

    fn room() -> SharedRoom {
        let mut room = SharedRoom::new(RoomId("r".into()), "host".into());
        room.join("amy".into()).unwrap();
        room.join("bob".into()).unwrap();
        room
    }

    fn p(name: &str) -> ParticipantId {
        name.into()
    }

    #[test]
    fn everyone_can_control_and_each_command_bumps_the_version() {
        let mut room = room();
        let base = room.revision;
        let first = room
            .apply_control(
                &p("amy"),
                ControlCommand::Enqueue {
                    track: track("a", 100_000),
                },
                0,
            )
            .unwrap();
        assert!(first > base);
        let second = room
            .apply_control(&p("bob"), ControlCommand::Play, 1_000)
            .unwrap();
        assert!(second > first);
        assert!(room.is_playing);
        assert_eq!(room.current_position_ms(3_500), 2_500);
    }

    #[test]
    fn pause_freezes_the_room_clock_and_play_resumes_from_it() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::Enqueue {
                track: track("a", 100_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(&p("host"), ControlCommand::Play, 0)
            .unwrap();
        room.apply_control(&p("amy"), ControlCommand::Pause, 4_000)
            .unwrap();
        assert_eq!(room.current_position_ms(60_000), 4_000);
        room.apply_control(&p("bob"), ControlCommand::Play, 10_000)
            .unwrap();
        assert_eq!(room.current_position_ms(11_000), 5_000);
    }

    #[test]
    fn host_only_and_delegated_modes_restrict_control() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::SetControlMode {
                mode: ControlMode::HostOnly,
            },
            0,
        )
        .unwrap();
        assert_eq!(
            room.apply_control(&p("amy"), ControlCommand::Play, 0),
            Err(SocialError::PermissionDenied)
        );
        assert_eq!(
            room.apply_control(
                &p("amy"),
                ControlCommand::SetControlMode {
                    mode: ControlMode::Everyone
                },
                0
            ),
            Err(SocialError::PermissionDenied)
        );

        room.apply_control(
            &p("host"),
            ControlCommand::SetControlMode {
                mode: ControlMode::Delegated,
            },
            0,
        )
        .unwrap();
        room.apply_control(
            &p("host"),
            ControlCommand::GrantControl {
                participant: p("amy"),
            },
            0,
        )
        .unwrap();
        assert!(room
            .apply_control(
                &p("amy"),
                ControlCommand::Enqueue {
                    track: track("a", 1_000)
                },
                0
            )
            .is_ok());
        assert_eq!(
            room.apply_control(&p("bob"), ControlCommand::Pause, 0),
            Err(SocialError::PermissionDenied)
        );
        room.apply_control(
            &p("host"),
            ControlCommand::RevokeControl {
                participant: p("amy"),
            },
            0,
        )
        .unwrap();
        assert!(!room.can_control(&p("amy")));
    }

    #[test]
    fn rejected_commands_do_not_change_the_version() {
        let mut room = room();
        room.control_mode = ControlMode::HostOnly;
        let before = room.revision;
        assert!(room
            .apply_control(&p("amy"), ControlCommand::Play, 0)
            .is_err());
        assert!(room
            .apply_control(&p("stranger"), ControlCommand::Play, 0)
            .is_err());
        assert_eq!(room.revision, before);
    }

    #[test]
    fn anyone_can_add_personal_effects_even_in_host_only_mode() {
        let mut room = room();
        room.control_mode = ControlMode::HostOnly;
        let echo = LiveEffect::Echo {
            delay_ms: 300,
            feedback: 0.4,
            mix: 0.5,
        };
        room.apply_control(
            &p("bob"),
            ControlCommand::AddMyEffect {
                effect: echo.clone(),
            },
            0,
        )
        .unwrap();
        assert_eq!(room.participant_effects[&p("bob")].effects, vec![echo]);
        assert_eq!(
            room.apply_control(
                &p("bob"),
                ControlCommand::AddGlobalEffect {
                    effect: LiveEffect::PitchShift { semitones: 2.0 }
                },
                0
            ),
            Err(SocialError::PermissionDenied)
        );
        let invalid = LiveEffect::Echo {
            delay_ms: 9_999,
            feedback: 0.4,
            mix: 0.5,
        };
        assert_eq!(
            room.apply_control(
                &p("bob"),
                ControlCommand::AddMyEffect { effect: invalid },
                0
            ),
            Err(SocialError::InvalidEffect)
        );
        room.apply_control(&p("bob"), ControlCommand::ClearMyEffects, 0)
            .unwrap();
        assert!(!room.participant_effects.contains_key(&p("bob")));
    }

    #[test]
    fn queue_advances_on_skip_and_when_the_track_finishes() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::Enqueue {
                track: track("a", 10_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(
            &p("amy"),
            ControlCommand::Enqueue {
                track: track("b", 20_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(&p("host"), ControlCommand::Play, 0)
            .unwrap();
        assert!(!room.advance_if_finished(5_000));
        assert!(room.advance_if_finished(10_000));
        assert_eq!(room.current_track.as_ref().unwrap().id, "b".into());
        assert_eq!(room.current_position_ms(10_000), 0);
        room.apply_control(&p("bob"), ControlCommand::SkipToNext, 12_000)
            .unwrap();
        assert!(room.current_track.is_none());
        assert!(!room.is_playing);
    }

    #[test]
    fn seek_is_clamped_and_leaving_removes_control_rights() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::Enqueue {
                track: track("a", 10_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(
            &p("amy"),
            ControlCommand::Seek {
                position_ms: 99_000,
            },
            0,
        )
        .unwrap();
        assert_eq!(room.position_ms, 10_000);
        room.controllers.insert(p("bob"));
        room.leave(&p("bob")).unwrap();
        assert!(room.controllers.is_empty());
    }

    #[test]
    fn drift_correction_tolerates_small_nudges_medium_and_seeks_large() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::Enqueue {
                track: track("a", 600_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(&p("host"), ControlCommand::Play, 0)
            .unwrap();
        let now = 10_000;
        assert_eq!(room.correct_drift(10_050, now), DriftAction::InSync);
        assert_eq!(
            room.correct_drift(9_700, now),
            DriftAction::AdjustSpeed(1.04)
        );
        assert_eq!(
            room.correct_drift(10_500, now),
            DriftAction::AdjustSpeed(0.96)
        );
        assert_eq!(room.correct_drift(2_000, now), DriftAction::SeekTo(10_000));
    }

    #[test]
    fn paused_rooms_snap_listeners_to_the_paused_position() {
        let mut room = room();
        room.apply_control(
            &p("host"),
            ControlCommand::Enqueue {
                track: track("a", 600_000),
            },
            0,
        )
        .unwrap();
        room.apply_control(
            &p("host"),
            ControlCommand::Seek {
                position_ms: 30_000,
            },
            0,
        )
        .unwrap();
        assert_eq!(room.correct_drift(29_950, 5_000), DriftAction::InSync);
        assert_eq!(
            room.correct_drift(10_000, 5_000),
            DriftAction::SeekTo(30_000)
        );
    }
}
