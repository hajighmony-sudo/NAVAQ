//! Shared Listening Rooms with simultaneous control.
//!
//! Multiple participants listen to the same track in real time.
//! Each participant (or the host) can apply live effects such as echo,
//! reverb, vocal overlay or custom FX. The core owns only the logical
//! room state; networking, audio mixing and presence are host responsibilities.

use crate::model::Track;
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
            LiveEffect::VocalOverlay { volume, .. } => (0.0..=2.0).contains(volume),
            LiveEffect::Custom { name, .. } => !name.is_empty(),
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
        if !effect.is_valid() {
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
#[serde(tag = "command", rename_all = "snake_case")]
pub enum RoomCommand {
    SetTrack(Option<Track>),
    SetPlayback { position_ms: u64, is_playing: bool },
    SetGlobalEffect(LiveEffect),
    SetLocalEffect(LiveEffect),
    ClearLocalEffects,
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
    /// Maximum number of participants (0 = unlimited).
    pub max_participants: u32,
    /// Room is public and discoverable.
    pub is_public: bool,
    /// Optional human-readable name.
    pub name: Option<String>,
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
            max_participants: 0,
            is_public: false,
            name: None,
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
                self.set_track(track);
            }
            RoomCommand::SetPlayback {
                position_ms,
                is_playing,
            } => {
                self.position_ms = position_ms;
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
    #[error("only the room host can perform this action")]
    HostOnly,
    #[error("room changed; current revision is {current}")]
    RevisionConflict { current: u64 },
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
                RoomCommand::SetPlayback { position_ms: 2_000, is_playing: true },
            )
            .unwrap();
        assert!(next > revision);
        assert_eq!(
            room.apply_command(&guest, revision, RoomCommand::ClearLocalEffects),
            Err(SocialError::RevisionConflict { current: next })
        );
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
