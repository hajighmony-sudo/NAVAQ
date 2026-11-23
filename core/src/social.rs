//! Shared Listening Rooms with simultaneous control.
//!
//! Multiple participants listen to the same track in real time.
//! Each participant (or the host) can apply live effects such as echo,
//! reverb, vocal overlay or custom FX. The core owns only the logical
//! room state; networking, audio mixing and presence are host responsibilities.

use crate::model::Track;
use crate::room_control::ControlMode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Unique identifier for a shared listening room.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoomId(pub String);

impl From<&str> for RoomId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Participant identity inside a room.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParticipantId(pub String);

impl From<&str> for ParticipantId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Live audio effect that can be applied by any participant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEffect {
    /// Simple delay/echo.
    Echo {
        delay_ms: u32,
        feedback: f32, // 0.0..=1.0
        mix: f32,      // 0.0..=1.0
    },
    /// Room reverb.
    Reverb {
        room_size: f32, // 0.0..=1.0
        damping: f32,
        mix: f32,
    },
    /// Pitch shift in semitones.
    PitchShift { semitones: f32 },
    /// Temporary vocal overlay (karaoke-style or party vocal).
    VocalOverlay {
        /// Relative volume of the added vocal stem.
        volume: f32,
        /// Optional source of the vocal (host-provided URL or local id).
        source: Option<String>,
    },
    /// Custom named effect with free-form parameters (host interprets).
    Custom {
        name: String,
        params: BTreeMap<String, f32>,
    },
}

impl LiveEffect {
    pub fn is_valid(&self) -> bool {
        match self {
            LiveEffect::Echo {
                delay_ms,
                feedback,
                mix,
            } => *delay_ms <= 2000 && (0.0..=1.0).contains(feedback) && (0.0..=1.0).contains(mix),
            LiveEffect::Reverb {
                room_size,
                damping,
                mix,
            } => {
                (0.0..=1.0).contains(room_size)
                    && (0.0..=1.0).contains(damping)
                    && (0.0..=1.0).contains(mix)
            }
            LiveEffect::PitchShift { semitones } => (-24.0..=24.0).contains(semitones),
            LiveEffect::VocalOverlay { volume, source } => {
                (0.0..=2.0).contains(volume)
                    && source.as_ref().map_or(true, |value| {
                        !value.trim().is_empty() && value.len() <= 2048
                    })
            }
            LiveEffect::Custom { name, params } => {
                !name.trim().is_empty()
                    && name.len() <= 64
                    && params.len() <= 32
                    && params
                        .values()
                        .all(|value| value.is_finite() && value.abs() <= 1000.0)
            }
        }
    }
}

/// Per-participant effect stack (order matters for mixing).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectStack {
    pub effects: Vec<LiveEffect>,
}

impl EffectStack {
    pub fn push(&mut self, effect: LiveEffect) -> Result<(), SocialError> {
        if !effect.is_valid() || self.effects.len() >= 16 {
            return Err(SocialError::InvalidEffect);
        }
        self.effects.push(effect);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.effects.clear();
    }

    pub fn remove_last(&mut self) -> Option<LiveEffect> {
        self.effects.pop()
    }
}

/// Revision-checked commands accepted by a shared room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum RoomCommand {
    SetTrack(Option<Track>),
    SetPlayback { position_ms: u64, is_playing: bool },
    SetGlobalEffect(LiveEffect),
    SetLocalEffect(LiveEffect),
    ClearLocalEffects,
    SetSharedEffect(LiveEffect),
    ClearSharedEffects,
}

/// Policy for correcting clock drift between a room host and a listener.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoomSyncPolicy {
    /// Drift below this threshold is ignored.
    pub soft_drift_ms: u64,
    /// Drift above this threshold causes a hard seek.
    pub hard_drift_ms: u64,
    /// Maximum relative playback-rate correction (0.03 = ±3%).
    pub max_rate_adjustment: f32,
}

impl Default for RoomSyncPolicy {
    fn default() -> Self {
        Self {
            soft_drift_ms: 40,
            hard_drift_ms: 250,
            max_rate_adjustment: 0.03,
        }
    }
}

impl RoomSyncPolicy {
    pub fn is_valid(&self) -> bool {
        self.soft_drift_ms < self.hard_drift_ms
            && self.hard_drift_ms > 0
            && self.max_rate_adjustment.is_finite()
            && (0.001..=0.1).contains(&self.max_rate_adjustment)
    }
}

/// Action a listener should take to follow the room's authoritative transport clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoomSyncDecision {
    InSync,
    AdjustRate { speed: f32 },
    Seek { position_ms: u64 },
}

/// Estimate host position after one-way latency and choose a gentle correction or seek.
/// `current_speed` is the listener's nominal user-selected speed; `InSync` means
/// the host should restore that nominal speed if a previous correction was active.
/// Call while the host is playing; paused-room transport should be mirrored directly.
pub fn calculate_room_sync(
    local_position_ms: u64,
    host_position_ms: u64,
    one_way_latency_ms: u64,
    current_speed: f32,
    policy: RoomSyncPolicy,
) -> Result<RoomSyncDecision, SocialError> {
    if !policy.is_valid() || !current_speed.is_finite() || !(0.25..=4.0).contains(&current_speed) {
        return Err(SocialError::InvalidSyncPolicy);
    }
    let expected_position = host_position_ms.saturating_add(one_way_latency_ms);
    let drift = expected_position as i128 - local_position_ms as i128;
    let absolute_drift = drift.unsigned_abs();
    if absolute_drift <= policy.soft_drift_ms as u128 {
        return Ok(RoomSyncDecision::InSync);
    }
    if absolute_drift >= policy.hard_drift_ms as u128 {
        return Ok(RoomSyncDecision::Seek {
            position_ms: expected_position,
        });
    }
    let correction = (drift as f32 / policy.hard_drift_ms as f32) * policy.max_rate_adjustment;
    let speed = (current_speed + correction).clamp(0.25, 4.0);
    Ok(RoomSyncDecision::AdjustRate { speed })
}

/// Snapshot of a shared room at a moment in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedRoom {
    pub id: RoomId,
    pub host: ParticipantId,
    pub participants: BTreeSet<ParticipantId>,
    /// Current track being listened to (None = waiting).
    pub current_track: Option<Track>,
    /// Logical position that everyone should be synced to (host is source of truth).
    pub position_ms: u64,
    /// Whether playback is currently running.
    pub is_playing: bool,
    /// Global effects applied to the whole room (host controlled).
    pub global_effects: EffectStack,
    /// Per-participant private effects (only audible to that user or as they choose).
    pub participant_effects: BTreeMap<ParticipantId, EffectStack>,
    /// Effects contributed by participants and audible to the whole room.
    #[serde(default)]
    pub shared_effects: BTreeMap<ParticipantId, EffectStack>,
    /// Maximum number of participants (0 = unlimited).
    pub max_participants: u32,
    /// Room is public and discoverable.
    pub is_public: bool,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Who may control playback and the shared queue.
    #[serde(default)]
    pub control_mode: ControlMode,
    /// Participants allowed to control playback when `control_mode` is `Delegated`.
    #[serde(default)]
    pub controllers: BTreeSet<ParticipantId>,
    /// Host clock time at which `position_ms` was valid while playing (`None` = not anchored).
    #[serde(default)]
    pub anchor_at_ms: Option<u64>,
    /// Upcoming tracks shared by everyone in the room.
    #[serde(default)]
    pub queue: Vec<Track>,
    #[serde(default)]
    pub revision: u64,
}

impl SharedRoom {
    pub fn new(id: RoomId, host: ParticipantId) -> Self {
        let mut participants = BTreeSet::new();
        participants.insert(host.clone());
        Self {
            id,
            host,
            participants,
            current_track: None,
            position_ms: 0,
            is_playing: false,
            global_effects: EffectStack::default(),
            participant_effects: BTreeMap::new(),
            shared_effects: BTreeMap::new(),
            max_participants: 0,
            is_public: false,
            name: None,
            control_mode: ControlMode::default(),
            controllers: BTreeSet::new(),
            anchor_at_ms: None,
            queue: Vec::new(),
            revision: 0,
        }
    }

    pub fn join(&mut self, participant: ParticipantId) -> Result<(), SocialError> {
        if self.participants.contains(&participant) {
            return Ok(());
        }
        if self.max_participants > 0 && self.participants.len() as u32 >= self.max_participants {
            return Err(SocialError::RoomFull);
        }
        self.participants.insert(participant);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn leave(&mut self, participant: &ParticipantId) -> Result<(), SocialError> {
        if !self.participants.remove(participant) {
            return Err(SocialError::NotInRoom);
        }
        self.participant_effects.remove(participant);
        self.controllers.remove(participant);
        self.shared_effects.remove(participant);
        self.revision = self.revision.saturating_add(1);
        // If the host left, promote the next participant (or close).
        if &self.host == participant {
            if let Some(next) = self.participants.iter().next().cloned() {
                self.host = next;
            }
        }
        Ok(())
    }

    pub fn set_track(&mut self, track: Option<Track>) {
        self.current_track = track;
        self.position_ms = 0;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn sync_position(&mut self, position_ms: u64, is_playing: bool) {
        self.position_ms = position_ms;
        self.is_playing = is_playing;
        self.anchor_at_ms = None;
        self.revision = self.revision.saturating_add(1);
    }

    /// Apply a revision-checked command. Transport is shared; track and global mix are host-only.
    pub fn apply_command(
        &mut self,
        actor: &ParticipantId,
        expected_revision: u64,
        command: RoomCommand,
    ) -> Result<u64, SocialError> {
        if !self.participants.contains(actor) {
            return Err(SocialError::NotInRoom);
        }
        if expected_revision != self.revision {
            return Err(SocialError::RevisionConflict {
                current: self.revision,
            });
        }
        match command {
            RoomCommand::SetTrack(track) => {
                if !self.is_host(actor) {
                    return Err(SocialError::HostOnly);
                }
                if track.as_ref().is_some_and(|value| !value.is_valid()) {
                    return Err(SocialError::InvalidTrack);
                }
                self.set_track(track);
            }
            RoomCommand::SetPlayback {
                position_ms,
                is_playing,
            } => {
                if is_playing && self.current_track.is_none() {
                    return Err(SocialError::NoTrack);
                }
                self.position_ms = self
                    .current_track
                    .as_ref()
                    .and_then(|track| track.duration_ms)
                    .map_or(position_ms, |duration| position_ms.min(duration));
                self.is_playing = is_playing;
                self.revision = self.revision.saturating_add(1);
            }
            RoomCommand::SetGlobalEffect(effect) => {
                if !self.is_host(actor) {
                    return Err(SocialError::HostOnly);
                }
                self.apply_global_effect(effect)?;
                self.revision = self.revision.saturating_add(1);
            }
            RoomCommand::SetLocalEffect(effect) => {
                self.apply_participant_effect(actor, effect)?;
                self.revision = self.revision.saturating_add(1);
            }
            RoomCommand::ClearLocalEffects => {
                self.participant_effects.remove(actor);
                self.revision = self.revision.saturating_add(1);
            }
            RoomCommand::SetSharedEffect(effect) => {
                self.shared_effects
                    .entry(actor.clone())
                    .or_default()
                    .push(effect)?;
                self.revision = self.revision.saturating_add(1);
            }
            RoomCommand::ClearSharedEffects => {
                self.shared_effects.remove(actor);
                self.revision = self.revision.saturating_add(1);
            }
        }
        Ok(self.revision)
    }
    pub fn apply_global_effect(&mut self, effect: LiveEffect) -> Result<(), SocialError> {
        self.global_effects.push(effect)
    }

    pub fn apply_participant_effect(
        &mut self,
        participant: &ParticipantId,
        effect: LiveEffect,
    ) -> Result<(), SocialError> {
        if !self.participants.contains(participant) {
            return Err(SocialError::NotInRoom);
        }
        self.participant_effects
            .entry(participant.clone())
            .or_default()
            .push(effect)
    }

    pub fn clear_global_effects(&mut self) {
        self.global_effects.clear();
    }

    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    pub fn is_host(&self, id: &ParticipantId) -> bool {
        &self.host == id
    }
}

/// In-memory store of active rooms (host persists and synchronises).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomStore {
    rooms: BTreeMap<RoomId, SharedRoom>,
}

impl RoomStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, id: RoomId, host: ParticipantId) -> Result<&SharedRoom, SocialError> {
        if self.rooms.contains_key(&id) {
            return Err(SocialError::RoomExists);
        }
        let room = SharedRoom::new(id.clone(), host);
        self.rooms.insert(id.clone(), room);
        Ok(self.rooms.get(&id).unwrap())
    }

    pub fn get(&self, id: &RoomId) -> Option<&SharedRoom> {
        self.rooms.get(id)
    }

    pub fn get_mut(&mut self, id: &RoomId) -> Option<&mut SharedRoom> {
        self.rooms.get_mut(id)
    }

    pub fn remove(&mut self, id: &RoomId) -> Option<SharedRoom> {
        self.rooms.remove(id)
    }

    pub fn public_rooms(&self) -> Vec<&SharedRoom> {
        self.rooms.values().filter(|r| r.is_public).collect()
    }

    pub fn rooms_for_participant(&self, participant: &ParticipantId) -> Vec<&SharedRoom> {
        self.rooms
            .values()
            .filter(|r| r.participants.contains(participant))
            .collect()
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SocialError {
    #[error("room already exists")]
    RoomExists,
    #[error("room is full")]
    RoomFull,
    #[error("participant is not in the room")]
    NotInRoom,
    #[error("invalid live effect parameters")]
    InvalidEffect,
    #[error("room not found")]
    RoomNotFound,
    #[error("participant is not allowed to do this in the room")]
    PermissionDenied,
    #[error("only the room host can perform this action")]
    HostOnly,
    #[error("room changed; current revision is {current}")]
    RevisionConflict { current: u64 },
    #[error("track metadata or source is invalid")]
    InvalidTrack,
    #[error("a room needs a track before playback can start")]
    NoTrack,
    #[error("invalid room synchronization policy or playback speed")]
    InvalidSyncPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;

    fn sample_track() -> Track {
        Track {
            id: "t1".into(),
            title: "Party Track".into(),
            artist: Some("DJ".into()),
            album: None,
            duration_ms: Some(180_000),
            source: AudioSource::LocalFile("/party.mp3".into()),
        }
    }

    #[test]
    fn room_sync_uses_soft_correction_and_hard_seek_thresholds() {
        let policy = RoomSyncPolicy::default();
        assert_eq!(
            calculate_room_sync(1_020, 1_000, 0, 1.0, policy).unwrap(),
            RoomSyncDecision::InSync
        );
        match calculate_room_sync(1_000, 1_100, 0, 1.0, policy).unwrap() {
            RoomSyncDecision::AdjustRate { speed } => assert!(speed > 1.0 && speed <= 1.03),
            other => panic!("expected gentle rate correction, got {other:?}"),
        }
        assert_eq!(
            calculate_room_sync(1_000, 1_400, 0, 1.0, policy).unwrap(),
            RoomSyncDecision::Seek { position_ms: 1_400 }
        );
    }

    #[test]
    fn room_sync_accounts_for_latency_and_rejects_invalid_policy() {
        let policy = RoomSyncPolicy::default();
        assert_eq!(
            calculate_room_sync(1_100, 1_000, 100, 1.0, policy).unwrap(),
            RoomSyncDecision::InSync
        );
        let invalid = RoomSyncPolicy {
            hard_drift_ms: 0,
            ..policy
        };
        assert_eq!(
            calculate_room_sync(0, 0, 0, 1.0, invalid),
            Err(SocialError::InvalidSyncPolicy)
        );
    }

    #[test]
    fn create_join_leave_and_effects() {
        let mut store = RoomStore::new();
        let room = store.create("room1".into(), "host".into()).unwrap().clone();
        assert_eq!(room.participant_count(), 1);

        let room = store.get_mut(&"room1".into()).unwrap();
        room.join("user2".into()).unwrap();
        room.join("user3".into()).unwrap();
        assert_eq!(room.participant_count(), 3);

        room.apply_global_effect(LiveEffect::Echo {
            delay_ms: 300,
            feedback: 0.4,
            mix: 0.3,
        })
        .unwrap();
        room.apply_participant_effect(&"user2".into(), LiveEffect::PitchShift { semitones: 2.0 })
            .unwrap();

        assert_eq!(room.global_effects.effects.len(), 1);
        assert_eq!(
            room.participant_effects
                .get(&"user2".into())
                .unwrap()
                .effects
                .len(),
            1
        );

        room.leave(&"user2".into()).unwrap();
        assert!(!room.participants.contains(&"user2".into()));
        assert!(!room.participant_effects.contains_key(&"user2".into()));
    }

    #[test]
    fn host_leave_promotes_next() {
        let mut store = RoomStore::new();
        store.create("r".into(), "h".into()).unwrap();
        let room = store.get_mut(&"r".into()).unwrap();
        room.join("a".into()).unwrap();
        room.join("b".into()).unwrap();
        room.leave(&"h".into()).unwrap();
        assert_ne!(room.host.0, "h");
        assert!(room.participants.contains(&room.host));
    }

    #[test]
    fn invalid_effect_rejected() {
        let mut room = SharedRoom::new("r".into(), "h".into());
        assert_eq!(
            room.apply_global_effect(LiveEffect::Echo {
                delay_ms: 5000,
                feedback: 0.5,
                mix: 0.5,
            }),
            Err(SocialError::InvalidEffect)
        );
    }

    #[test]
    fn room_commands_enforce_host_permissions_and_revision() {
        let mut room = SharedRoom::new("r".into(), "host".into());
        room.join("guest".into()).unwrap();
        room.set_track(Some(sample_track()));
        let guest = ParticipantId::from("guest");
        let revision = room.revision;
        assert_eq!(
            room.apply_command(&guest, revision, RoomCommand::SetTrack(None)),
            Err(SocialError::HostOnly)
        );
        let next = room
            .apply_command(
                &guest,
                revision,
                RoomCommand::SetPlayback {
                    position_ms: 999_999,
                    is_playing: true,
                },
            )
            .unwrap();
        assert!(next > revision);
        assert_eq!(room.position_ms, 180_000);
        assert_eq!(
            room.apply_command(&guest, revision, RoomCommand::ClearLocalEffects),
            Err(SocialError::RevisionConflict { current: next })
        );
    }

    #[test]
    fn room_cannot_start_without_a_track_and_rejects_invalid_track() {
        let mut room = SharedRoom::new("r".into(), "host".into());
        let host = ParticipantId::from("host");
        assert_eq!(
            room.apply_command(
                &host,
                room.revision,
                RoomCommand::SetPlayback {
                    position_ms: 100,
                    is_playing: true,
                },
            ),
            Err(SocialError::NoTrack)
        );
        let mut invalid = sample_track();
        invalid.title.clear();
        assert_eq!(
            room.apply_command(&host, room.revision, RoomCommand::SetTrack(Some(invalid)),),
            Err(SocialError::InvalidTrack)
        );
    }

    #[test]
    fn participants_can_contribute_and_remove_room_wide_effects() {
        let mut room = SharedRoom::new("r".into(), "host".into());
        room.join("guest".into()).unwrap();
        let guest = ParticipantId::from("guest");
        let revision = room.revision;
        room.apply_command(
            &guest,
            revision,
            RoomCommand::SetSharedEffect(LiveEffect::Echo {
                delay_ms: 250,
                feedback: 0.2,
                mix: 0.3,
            }),
        )
        .unwrap();
        assert_eq!(room.shared_effects.get(&guest).unwrap().effects.len(), 1);
        room.leave(&guest).unwrap();
        assert!(!room.shared_effects.contains_key(&guest));
    }
    #[test]
    fn duplicate_join_does_not_consume_room_capacity() {
        let mut room = SharedRoom::new("r".into(), "host".into());
        room.max_participants = 2;
        room.join("guest".into()).unwrap();
        room.join("guest".into()).unwrap();
        assert_eq!(room.participant_count(), 2);
        assert_eq!(room.join("third".into()), Err(SocialError::RoomFull));
    }
    #[test]
    fn set_track_and_sync() {
        let mut room = SharedRoom::new("r".into(), "h".into());
        room.set_track(Some(sample_track()));
        room.sync_position(42_000, true);
        assert!(room.is_playing);
        assert_eq!(room.position_ms, 42_000);
        assert_eq!(room.current_track.as_ref().unwrap().id.0, "t1");
    }
}
