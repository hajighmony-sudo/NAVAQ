//! Music DNA — taste profiling and similarity matching.
//!
//! Builds a compact taste signature from play history, ratings and optional
//! genre/tempo tags supplied by the host. Matching is pure cosine / weighted
//! overlap so the host can rank other users or generate collaborative playlists.

use crate::history::PlayHistory;
use crate::model::TrackId;
use crate::ratings::UserRatings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Sparse feature vector representing musical taste.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TasteProfile {
    /// Artist preference weights (normalized later).
    pub artists: BTreeMap<String, f32>,
    /// Genre / style weights.
    pub genres: BTreeMap<String, f32>,
    /// Average preferred tempo (BPM). None if unknown.
    pub avg_tempo: Option<f32>,
    /// Preferred energy / danceability proxy (0.0..=1.0).
    pub energy: Option<f32>,
    /// Total plays considered when building the profile.
    pub sample_size: u32,
}

impl TasteProfile {
    pub fn is_valid(&self) -> bool {
        self.artists
            .values()
            .chain(self.genres.values())
            .all(|v| v.is_finite() && *v >= 0.0)
            && self
                .avg_tempo
                .map_or(true, |v| v.is_finite() && (20.0..=300.0).contains(&v))
            && self
                .energy
                .map_or(true, |v| v.is_finite() && (0.0..=1.0).contains(&v))
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Merge another profile with a weight (useful for time-decayed updates).
    pub fn merge(&mut self, other: &TasteProfile, weight: f32) {
        if !self.is_valid()
            || !weight.is_finite()
            || weight <= 0.0
            || weight > 100.0
            || !other.is_valid()
        {
            return;
        }
        for (k, &v) in &other.artists {
            *self.artists.entry(k.clone()).or_insert(0.0) += v * weight;
        }
        for (k, &v) in &other.genres {
            *self.genres.entry(k.clone()).or_insert(0.0) += v * weight;
        }
        if let Some(t) = other.avg_tempo {
            self.avg_tempo = Some(match self.avg_tempo {
                Some(cur) => (cur + t * weight) / (1.0 + weight),
                None => t,
            });
        }
        if let Some(e) = other.energy {
            self.energy = Some(match self.energy {
                Some(cur) => (cur + e * weight) / (1.0 + weight),
                None => e,
            });
        }
        self.sample_size = self.sample_size.saturating_add(other.sample_size);
    }

    /// L2-normalize artist and genre vectors for cosine similarity.
    pub fn normalize(&mut self) {
        normalize_map(&mut self.artists);
        normalize_map(&mut self.genres);
    }

    /// Cosine similarity on the combined artist+genre space (0.0..=1.0).
    pub fn similarity(&self, other: &TasteProfile) -> f32 {
        if !self.is_valid() || !other.is_valid() {
            return 0.0;
        }
        let mut left = self.clone();
        let mut right = other.clone();
        left.normalize();
        right.normalize();
        let artist_sim = cosine(&left.artists, &right.artists);
        let genre_sim = cosine(&left.genres, &right.genres);
        // Weighted blend; genres often more stable for matching.
        let mut score = 0.45 * artist_sim + 0.55 * genre_sim;

        // Soft tempo agreement.
        if let (Some(a), Some(b)) = (left.avg_tempo, right.avg_tempo) {
            let diff = (a - b).abs();
            let tempo_factor = (1.0 - (diff / 40.0).min(1.0)).max(0.0);
            score = 0.85 * score + 0.15 * tempo_factor;
        }
        if let (Some(a), Some(b)) = (left.energy, right.energy) {
            let energy_factor = 1.0 - (a - b).abs();
            score = 0.9 * score + 0.1 * energy_factor;
        }
        score.clamp(0.0, 1.0)
    }
}

fn normalize_map(map: &mut BTreeMap<String, f32>) {
    let norm: f32 = map.values().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for v in map.values_mut() {
            *v /= norm;
        }
    }
}

fn cosine(a: &BTreeMap<String, f32>, b: &BTreeMap<String, f32>) -> f32 {
    let mut dot = 0.0f32;
    for (k, &va) in a {
        if let Some(&vb) = b.get(k) {
            dot += va * vb;
        }
    }
    // Already normalized → cosine is just the dot product.
    dot.clamp(0.0, 1.0)
}

/// Builder that turns listening data into a TasteProfile.
#[derive(Debug, Clone, Default)]
pub struct DnaBuilder {
    profile: TasteProfile,
}

impl DnaBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an artist listen (weight usually 1.0 or play-count based).
    pub fn add_artist(&mut self, artist: &str, weight: f32) {
        if weight.is_finite() && weight > 0.0 && !artist.trim().is_empty() {
            *self.profile.artists.entry(artist.to_owned()).or_insert(0.0) += weight;
            self.profile.sample_size = self.profile.sample_size.saturating_add(1);
        }
    }

    pub fn add_genre(&mut self, genre: &str, weight: f32) {
        if weight.is_finite() && weight > 0.0 && !genre.trim().is_empty() {
            *self.profile.genres.entry(genre.to_owned()).or_insert(0.0) += weight;
        }
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        if bpm.is_finite() && (20.0..=300.0).contains(&bpm) {
            self.profile.avg_tempo = Some(match self.profile.avg_tempo {
                Some(cur) => (cur + bpm) / 2.0,
                None => bpm,
            });
        }
    }

    pub fn set_energy(&mut self, energy: f32) {
        if (0.0..=1.0).contains(&energy) {
            self.profile.energy = Some(energy);
        }
    }

    /// Convenience: boost from a UserRatings + PlayHistory pair.
    /// The host must supply artist/genre lookup for each TrackId.
    pub fn from_history_and_ratings<FArt, FGen>(
        history: &PlayHistory,
        ratings: &UserRatings,
        artist_of: FArt,
        genre_of: FGen,
    ) -> TasteProfile
    where
        FArt: Fn(&TrackId) -> Option<String>,
        FGen: Fn(&TrackId) -> Option<String>,
    {
        let mut builder = DnaBuilder::new();

        // Play counts give base weight.
        for (id, count) in history.most_played(500) {
            let w = (count as f32).sqrt(); // dampen extreme repeats
            if let Some(artist) = artist_of(id) {
                builder.add_artist(&artist, w);
            }
            if let Some(genre) = genre_of(id) {
                builder.add_genre(&genre, w * 0.8);
            }
        }

        // Favorites and high ratings amplify.
        for id in ratings.favorites() {
            if let Some(artist) = artist_of(id) {
                builder.add_artist(&artist, 3.0);
            }
            if let Some(genre) = genre_of(id) {
                builder.add_genre(&genre, 2.5);
            }
        }
        for id in ratings.rated_at_least(4) {
            if let Some(artist) = artist_of(id) {
                builder.add_artist(&artist, 1.5);
            }
        }

        let mut profile = builder.build();
        profile.normalize();
        profile
    }

    pub fn build(self) -> TasteProfile {
        self.profile
    }
}

/// Result of matching one profile against a set of candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchResult {
    pub participant_id: String,
    pub score: f32,
}

/// A candidate is discoverable only after explicitly opting in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasteCandidate {
    pub participant_id: String,
    pub profile: TasteProfile,
    pub discovery_enabled: bool,
}

/// Rank only opted-in profiles with enough listening history to avoid weak matches.
pub fn rank_opted_in_matches(
    target: &TasteProfile,
    candidates: &[TasteCandidate],
    min_score: f32,
    min_sample_size: u32,
) -> Vec<MatchResult> {
    if !target.is_valid() || !min_score.is_finite() {
        return Vec::new();
    }
    let min_score = min_score.clamp(0.0, 1.0);
    let mut results: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.discovery_enabled
                && candidate.profile.sample_size >= min_sample_size
                && candidate.profile.is_valid()
        })
        .map(|candidate| MatchResult {
            participant_id: candidate.participant_id.clone(),
            score: target.similarity(&candidate.profile),
        })
        .filter(|result| result.score >= min_score.clamp(0.0, 1.0))
        .collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.participant_id.cmp(&b.participant_id))
    });
    results
}
/// Per-user affinity scores for a track, supplied by the host's catalog/ranking layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollaborativeTrackCandidate {
    pub track_id: TrackId,
    /// Scores are keyed by participant id and must be in 0.0..=1.0.
    pub participant_scores: BTreeMap<String, f32>,
}

/// Consensus ranking for a track in a multi-user playlist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollaborativeTrackResult {
    pub track_id: TrackId,
    /// Mean affinity multiplied by participant coverage, in 0.0..=1.0.
    pub consensus_score: f32,
    /// Fraction of requested participants for whom an affinity was supplied.
    pub coverage: f32,
}

/// Rank tracks for a shared playlist without allowing one listener's score to
/// completely dominate. Candidates must meet the requested participant coverage;
/// missing participants lower the consensus score. The host remains responsible
/// for obtaining each participant's scores from their consented taste profile.
pub fn rank_collaborative_tracks(
    participant_ids: &[String],
    candidates: &[CollaborativeTrackCandidate],
    min_coverage: f32,
    limit: usize,
) -> Vec<CollaborativeTrackResult> {
    if participant_ids.is_empty()
        || participant_ids.len() > 100
        || !min_coverage.is_finite()
        || !(0.0..=1.0).contains(&min_coverage)
        || limit == 0
    {
        return Vec::new();
    }

    let mut participants = BTreeSet::new();
    if participant_ids
        .iter()
        .any(|id| id.trim().is_empty() || id.len() > 128 || !participants.insert(id.as_str()))
    {
        return Vec::new();
    }

    let required = ((participant_ids.len() as f32 * min_coverage).ceil() as usize).max(1);
    let mut seen_tracks = BTreeSet::new();
    let mut results = Vec::new();

    for candidate in candidates {
        if candidate.track_id.0.trim().is_empty()
            || candidate.track_id.0.len() > 256
            || !seen_tracks.insert(candidate.track_id.clone())
            || candidate.participant_scores.iter().any(|(id, score)| {
                !participants.contains(id.as_str())
                    || !score.is_finite()
                    || !(0.0..=1.0).contains(score)
            })
        {
            continue;
        }

        let scores: Vec<f32> = participant_ids
            .iter()
            .filter_map(|id| candidate.participant_scores.get(id).copied())
            .collect();
        if scores.len() < required {
            continue;
        }

        let coverage = scores.len() as f32 / participant_ids.len() as f32;
        let mean = scores.iter().sum::<f32>() / scores.len() as f32;
        results.push(CollaborativeTrackResult {
            track_id: candidate.track_id.clone(),
            consensus_score: (mean * coverage).clamp(0.0, 1.0),
            coverage,
        });
    }

    results.sort_by(|a, b| {
        b.consensus_score
            .partial_cmp(&a.consensus_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.track_id.cmp(&b.track_id))
    });
    results.truncate(limit.min(500));
    results
}

/// Rank candidates by similarity to a target profile.
pub fn rank_matches(
    target: &TasteProfile,
    candidates: &[(String, TasteProfile)],
    min_score: f32,
) -> Vec<MatchResult> {
    if !target.is_valid() || !min_score.is_finite() {
        return Vec::new();
    }
    let min_score = min_score.clamp(0.0, 1.0);
    let mut results: Vec<_> = candidates
        .iter()
        .filter(|(_, profile)| profile.is_valid())
        .map(|(id, profile)| MatchResult {
            participant_id: id.clone(),
            score: target.similarity(profile),
        })
        .filter(|r| r.score >= min_score)
        .collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.participant_id.cmp(&b.participant_id))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_normalizes_raw_vectors_and_rejects_nan() {
        let mut a = TasteProfile::default();
        a.genres.insert("Rock".into(), 10.0);
        let mut b = TasteProfile::default();
        b.genres.insert("Rock".into(), 1.0);
        assert!((a.similarity(&b) - 1.0).abs() < 1e-5);
        b.genres.insert("bad".into(), f32::NAN);
        assert_eq!(a.similarity(&b), 0.0);
    }

    #[test]
    fn opted_in_matching_respects_discovery_and_sample_threshold() {
        let mut profile = TasteProfile::default();
        profile.genres.insert("Jazz".into(), 1.0);
        profile.sample_size = 25;
        let candidates = vec![
            TasteCandidate {
                participant_id: "visible".into(),
                profile: profile.clone(),
                discovery_enabled: true,
            },
            TasteCandidate {
                participant_id: "private".into(),
                profile: profile.clone(),
                discovery_enabled: false,
            },
            TasteCandidate {
                participant_id: "new".into(),
                profile: {
                    let mut p = profile.clone();
                    p.sample_size = 1;
                    p
                },
                discovery_enabled: true,
            },
        ];
        let ranked = rank_opted_in_matches(&profile, &candidates, 0.5, 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].participant_id, "visible");
    }

    #[test]
    fn collaborative_playlist_rewards_consensus_and_coverage() {
        let participants = vec!["a".to_string(), "b".to_string()];
        let candidates = vec![
            CollaborativeTrackCandidate {
                track_id: "balanced".into(),
                participant_scores: BTreeMap::from([("a".into(), 0.8), ("b".into(), 0.8)]),
            },
            CollaborativeTrackCandidate {
                track_id: "one_sided".into(),
                participant_scores: BTreeMap::from([("a".into(), 1.0)]),
            },
        ];
        let ranked = rank_collaborative_tracks(&participants, &candidates, 0.5, 10);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].track_id, TrackId::from("balanced"));
        assert!((ranked[0].coverage - 1.0).abs() < 1e-6);
        assert!((ranked[1].coverage - 0.5).abs() < 1e-6);
        assert!(ranked[0].consensus_score > ranked[1].consensus_score);
    }

    #[test]
    fn collaborative_playlist_rejects_invalid_participants_and_scores() {
        let participants = vec!["a".to_string(), "a".to_string()];
        let candidates = vec![CollaborativeTrackCandidate {
            track_id: "track".into(),
            participant_scores: BTreeMap::from([("a".into(), f32::NAN)]),
        }];
        assert!(rank_collaborative_tracks(&participants, &candidates, 0.5, 10).is_empty());

        let participants = vec!["a".to_string()];
        assert!(rank_collaborative_tracks(&participants, &candidates, 0.5, 10).is_empty());
    }

    #[test]
    fn identical_profiles_score_one() {
        let mut a = TasteProfile::default();
        a.artists.insert("ArtistX".into(), 1.0);
        a.genres.insert("Electronic".into(), 1.0);
        a.normalize();

        let b = a.clone();
        assert!((a.similarity(&b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn orthogonal_profiles_score_near_zero() {
        let mut a = TasteProfile::default();
        a.artists.insert("A".into(), 1.0);
        a.normalize();

        let mut b = TasteProfile::default();
        b.artists.insert("Z".into(), 1.0);
        b.normalize();

        assert!(a.similarity(&b) < 0.01);
    }

    #[test]
    fn ranking_orders_by_score() {
        let mut target = TasteProfile::default();
        target.genres.insert("Rock".into(), 1.0);
        target.normalize();

        let mut close = TasteProfile::default();
        close.genres.insert("Rock".into(), 0.9);
        close.genres.insert("Indie".into(), 0.1);
        close.normalize();

        let mut far = TasteProfile::default();
        far.genres.insert("Classical".into(), 1.0);
        far.normalize();

        let ranked = rank_matches(&target, &[("u1".into(), close), ("u2".into(), far)], 0.0);
        assert_eq!(ranked[0].participant_id, "u1");
        assert!(ranked[0].score > ranked[1].score);
    }
}
