//! Taste matching on top of `music_dna`: explain why two people match, suggest new
//! friends, and build a shared playlist that all participants will enjoy.

use crate::model::{Track, TrackId};
use crate::music_dna::{rank_matches, TasteProfile};
use crate::text::normalize_for_search;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Someone with a similar taste, with the reasons for the match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FriendSuggestion {
    pub participant_id: String,
    pub score: f32,
    /// Artists both people like, strongest overlap first.
    pub shared_artists: Vec<String>,
    pub shared_genres: Vec<String>,
}

/// Overlapping keys of two weight maps, ranked by the weaker of the two weights.
fn overlap(a: &BTreeMap<String, f32>, b: &BTreeMap<String, f32>, limit: usize) -> Vec<String> {
    let theirs: BTreeMap<String, f32> = b
        .iter()
        .map(|(k, w)| (normalize_for_search(k), *w))
        .collect();
    let mut shared: Vec<(String, f32)> = a
        .iter()
        .filter_map(|(name, weight)| {
            theirs
                .get(&normalize_for_search(name))
                .map(|other| (name.clone(), weight.min(*other)))
        })
        .filter(|(_, w)| *w > 0.0)
        .collect();
    shared.sort_by(|x, y| {
        y.1.partial_cmp(&x.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.0.cmp(&y.0))
    });
    shared
        .into_iter()
        .take(limit)
        .map(|(name, _)| name)
        .collect()
}

/// Artists both profiles like, best overlap first (at most `limit`).
pub fn shared_artists(a: &TasteProfile, b: &TasteProfile, limit: usize) -> Vec<String> {
    overlap(&a.artists, &b.artists, limit)
}

/// Genres both profiles like, best overlap first (at most `limit`).
pub fn shared_genres(a: &TasteProfile, b: &TasteProfile, limit: usize) -> Vec<String> {
    overlap(&a.genres, &b.genres, limit)
}

/// Rank people by taste similarity, skipping anyone in `exclude` (friends, blocked, yourself).
/// Only scores of at least `min_score` are kept, at most `limit` suggestions are returned.
pub fn suggest_friends(
    target: &TasteProfile,
    candidates: &[(String, TasteProfile)],
    exclude: &[String],
    min_score: f32,
    limit: usize,
) -> Vec<FriendSuggestion> {
    let excluded: HashSet<&str> = exclude.iter().map(String::as_str).collect();
    let pool: Vec<(String, TasteProfile)> = candidates
        .iter()
        .filter(|(id, _)| !excluded.contains(id.as_str()))
        .cloned()
        .collect();
    rank_matches(target, &pool, min_score)
        .into_iter()
        .take(limit)
        .filter_map(|m| {
            let profile = pool
                .iter()
                .find(|(id, _)| *id == m.participant_id)
                .map(|(_, p)| p)?;
            Some(FriendSuggestion {
                shared_artists: shared_artists(target, profile, 5),
                shared_genres: shared_genres(target, profile, 3),
                participant_id: m.participant_id,
                score: m.score,
            })
        })
        .collect()
}

/// One person taking part in a collaborative playlist.
pub struct PlaylistParticipant<'a> {
    pub id: &'a str,
    pub profile: &'a TasteProfile,
    /// Tracks this person is willing to contribute (their library or favorites).
    pub tracks: &'a [Track],
}

/// Build a shared playlist from everyone's tracks.
///
/// A track is scored by the average affinity of all participants to its artist, so songs by
/// artists that several people love rise to the top; a track one person contributes still
/// counts, but a mutual favorite always beats it. Duplicate tracks (same id, or same title
/// and artist) are merged. The result avoids the same artist twice in a row when possible
/// and no single participant fills more than their fair share (rounded up) of the list.
pub fn build_shared_playlist(
    participants: &[PlaylistParticipant<'_>],
    max_tracks: usize,
) -> Vec<Track> {
    if participants.is_empty() || max_tracks == 0 {
        return Vec::new();
    }
    let affinity: Vec<BTreeMap<String, f32>> = participants
        .iter()
        .map(|p| {
            p.profile
                .artists
                .iter()
                .map(|(artist, weight)| (normalize_for_search(artist), *weight))
                .collect()
        })
        .collect();
    let max_weight: Vec<f32> = affinity
        .iter()
        .map(|a| a.values().cloned().fold(0.0_f32, f32::max))
        .collect();

    struct Candidate {
        track: Track,
        score: f32,
        owners: Vec<usize>,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen_ids: HashSet<TrackId> = HashSet::new();
    let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();

    for (owner, participant) in participants.iter().enumerate() {
        for track in participant.tracks {
            let name_key = format!(
                "{}\u{1f}{}",
                normalize_for_search(&track.title),
                normalize_for_search(track.artist.as_deref().unwrap_or(""))
            );
            if let Some(&index) = seen_names.get(&name_key) {
                if !candidates[index].owners.contains(&owner) {
                    candidates[index].owners.push(owner);
                }
                continue;
            }
            if !seen_ids.insert(track.id.clone()) {
                continue;
            }
            let artist_key = normalize_for_search(track.artist.as_deref().unwrap_or(""));
            let score = affinity
                .iter()
                .zip(&max_weight)
                .map(|(a, max)| {
                    let weight = a.get(&artist_key).copied().unwrap_or(0.0);
                    if *max > 0.0 {
                        weight / max
                    } else {
                        0.0
                    }
                })
                .sum::<f32>()
                / participants.len() as f32;
            seen_names.insert(name_key, candidates.len());
            candidates.push(Candidate {
                track: track.clone(),
                score,
                owners: vec![owner],
            });
        }
    }

    // Tracks owned by several people count extra: everyone has it in their library.
    for c in &mut candidates {
        c.score += 0.1 * (c.owners.len() as f32 - 1.0) / participants.len() as f32;
    }
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.track.id.0.cmp(&b.track.id.0))
    });

    let cap = max_tracks.div_ceil(participants.len());
    let mut taken = vec![0usize; participants.len()];
    let mut remaining: Vec<Candidate> = Vec::new();
    // Fair-share cap (a track owned by several people counts against its first owner).
    for c in candidates {
        let owner = c.owners[0];
        if taken[owner] < cap {
            taken[owner] += 1;
            remaining.push(c);
        }
    }
    remaining.truncate(max_tracks);

    // Spread artists: take the best remaining track whose artist differs from the last one.
    let mut ordered: Vec<Track> = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let last = ordered
            .last()
            .map(|t| normalize_for_search(t.artist.as_deref().unwrap_or("")));
        let pick = remaining
            .iter()
            .position(|c| {
                let artist = normalize_for_search(c.track.artist.as_deref().unwrap_or(""));
                artist.is_empty() || last.as_deref() != Some(artist.as_str())
            })
            .unwrap_or(0);
        ordered.push(remaining.remove(pick).track);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioSource;
    use crate::music_dna::DnaBuilder;

    fn profile(artists: &[(&str, f32)], genres: &[(&str, f32)]) -> TasteProfile {
        let mut b = DnaBuilder::new();
        for (a, w) in artists {
            b.add_artist(a, *w);
        }
        for (g, w) in genres {
            b.add_genre(g, *w);
        }
        b.build()
    }

    fn track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: Some(artist.into()),
            album: None,
            duration_ms: Some(200_000),
            source: AudioSource::LocalFile(format!("/{id}.mp3")),
        }
    }

    #[test]
    fn explains_shared_artists_and_genres() {
        let me = profile(
            &[("Queen", 5.0), ("ABBA", 2.0), ("Zed", 1.0)],
            &[("rock", 3.0)],
        );
        let you = profile(
            &[("queen", 4.0), ("ABBA", 3.0)],
            &[("Rock", 1.0), ("jazz", 1.0)],
        );
        assert_eq!(shared_artists(&me, &you, 5), ["Queen", "ABBA"]);
        assert_eq!(shared_genres(&me, &you, 5), ["rock"]);
        assert!(shared_artists(&me, &you, 1).len() == 1);
    }

    #[test]
    fn suggests_closest_people_excluding_friends() {
        let me = profile(&[("Queen", 5.0), ("ABBA", 2.0)], &[("rock", 3.0)]);
        let close = profile(&[("Queen", 4.0), ("ABBA", 2.0)], &[("rock", 2.0)]);
        let far = profile(&[("Mozart", 5.0)], &[("classical", 5.0)]);
        let friend = close.clone();
        let candidates = vec![
            ("close".to_string(), close),
            ("far".to_string(), far),
            ("friend".to_string(), friend),
        ];
        let out = suggest_friends(&me, &candidates, &["friend".to_string()], 0.5, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].participant_id, "close");
        assert!(out[0].score > 0.9);
        assert_eq!(out[0].shared_artists, ["Queen", "ABBA"]);
        assert!(suggest_friends(&me, &candidates, &[], 0.5, 1).len() == 1);
    }

    #[test]
    fn shared_playlist_prefers_mutual_favorites_and_merges_duplicates() {
        let a = profile(&[("Queen", 5.0), ("ABBA", 1.0)], &[]);
        let b = profile(&[("Queen", 4.0), ("Mozart", 3.0)], &[]);
        let a_tracks = vec![
            track("a1", "Bohemian", "Queen"),
            track("a2", "Waterloo", "ABBA"),
        ];
        let b_tracks = vec![
            track("b1", "bohemian", "queen"),
            track("b2", "Requiem", "Mozart"),
            track("b3", "Don't Stop", "Queen"),
        ];
        let playlist = build_shared_playlist(
            &[
                PlaylistParticipant {
                    id: "a",
                    profile: &a,
                    tracks: &a_tracks,
                },
                PlaylistParticipant {
                    id: "b",
                    profile: &b,
                    tracks: &b_tracks,
                },
            ],
            10,
        );
        let titles: Vec<_> = playlist.iter().map(|t| t.title.to_lowercase()).collect();
        assert_eq!(titles.iter().filter(|t| *t == "bohemian").count(), 1);
        // Both Queen tracks outrank the single-fan artists.
        assert!(titles[..2].contains(&"bohemian".to_string()));
        assert_eq!(playlist.len(), 4);
    }

    #[test]
    fn shared_playlist_avoids_back_to_back_artists_and_respects_fair_share() {
        let a = profile(&[("Queen", 5.0)], &[]);
        let b = profile(&[("ABBA", 5.0)], &[]);
        let a_tracks: Vec<_> = (0..5)
            .map(|i| track(&format!("q{i}"), &format!("Q{i}"), "Queen"))
            .collect();
        let b_tracks: Vec<_> = (0..5)
            .map(|i| track(&format!("b{i}"), &format!("B{i}"), "ABBA"))
            .collect();
        let playlist = build_shared_playlist(
            &[
                PlaylistParticipant {
                    id: "a",
                    profile: &a,
                    tracks: &a_tracks,
                },
                PlaylistParticipant {
                    id: "b",
                    profile: &b,
                    tracks: &b_tracks,
                },
            ],
            6,
        );
        assert_eq!(playlist.len(), 6);
        let queen = playlist
            .iter()
            .filter(|t| t.artist.as_deref() == Some("Queen"))
            .count();
        assert_eq!(queen, 3);
        for pair in playlist.windows(2) {
            assert_ne!(pair[0].artist, pair[1].artist);
        }
    }

    #[test]
    fn empty_inputs_give_empty_playlists() {
        assert!(build_shared_playlist(&[], 5).is_empty());
        let p = TasteProfile::default();
        let tracks = vec![track("a", "A", "X")];
        let one = [PlaylistParticipant {
            id: "a",
            profile: &p,
            tracks: &tracks,
        }];
        assert!(build_shared_playlist(&one, 0).is_empty());
        assert_eq!(build_shared_playlist(&one, 5).len(), 1);
    }
}
