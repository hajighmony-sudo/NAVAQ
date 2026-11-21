//! Music DNA — taste profiling and similarity matching.
//!
//! Builds a compact taste signature from play history, ratings and optional
//! genre/tempo tags supplied by the host. Matching is pure cosine / weighted
//! overlap so the host can rank other users or generate collaborative playlists.

use crate::history::PlayHistory;
use crate::model::TrackId;
use crate::ratings::UserRatings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
        self.artists.values().chain(self.genres.values()).all(|v| v.is_finite() && *v >= 0.0)
            && self.avg_tempo.is_none_or(|v| v.is_finite() && (20.0..=300.0).contains(&v))
            && self.energy.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Merge another profile with a weight (useful for time-decayed updates).
    pub fn merge(&mut self, other: &TasteProfile, weight: f32) {
        if !weight.is_finite() || weight <= 0.0 || !other.is_valid() {
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
        if !self.is_valid() || !other.is_valid() { return 0.0; }
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

/// Rank candidates by similarity to a target profile.
pub fn rank_matches(
    target: &TasteProfile,
    candidates: &[(String, TasteProfile)],
    min_score: f32,
) -> Vec<MatchResult> {
    let mut results: Vec<_> = candidates
        .iter()
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
