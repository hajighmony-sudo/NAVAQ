//! Listening-time tracker implementing the common scrobble rule: the track must be at
//! least 30 s long and played for half its length or 4 minutes, whichever comes first.

use crate::model::TrackId;

pub const MIN_TRACK_MS: u64 = 30_000;
pub const MAX_THRESHOLD_MS: u64 = 240_000;
/// Position jumps larger than this between callbacks count as seeks, not listening.
const MAX_TICK_GAP_MS: u64 = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrobbleTracker {
    track: Option<TrackId>,
    last_position_ms: u64,
    played_ms: u64,
    emitted: bool,
}

impl ScrobbleTracker {
    /// Begin tracking `track` from `position_ms`.
    pub fn start(&mut self, track: TrackId, position_ms: u64) {
        self.track = Some(track);
        self.last_position_ms = position_ms;
        self.played_ms = 0;
        self.emitted = false;
    }

    /// Move the baseline after a seek so the jump is not counted as listening.
    pub fn rebase(&mut self, position_ms: u64) {
        self.last_position_ms = position_ms;
    }

    pub fn played_ms(&self) -> u64 {
        self.played_ms
    }

    /// Feed a playing-position update. Returns true exactly once, when the track qualifies.
    pub fn record_position(&mut self, position_ms: u64, duration_ms: Option<u64>) -> bool {
        if self.track.is_none() || self.emitted {
            return false;
        }
        if position_ms >= self.last_position_ms {
            let delta = position_ms - self.last_position_ms;
            if delta <= MAX_TICK_GAP_MS {
                self.played_ms += delta;
            }
        }
        self.last_position_ms = position_ms;

        let Some(duration) = duration_ms.filter(|d| *d >= MIN_TRACK_MS) else {
            return false;
        };
        if self.played_ms >= (duration / 2).min(MAX_THRESHOLD_MS) {
            self.emitted = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tracker: &mut ScrobbleTracker, from: u64, to: u64, duration: u64) -> bool {
        let mut hit = false;
        let mut p = from;
        while p < to {
            p += 1_000;
            hit |= tracker.record_position(p, Some(duration));
        }
        hit
    }

    #[test]
    fn fires_once_at_half_of_a_short_track() {
        let mut t = ScrobbleTracker::default();
        t.start("a".into(), 0);
        assert!(!run(&mut t, 0, 59_000, 120_000));
        assert!(t.record_position(60_000, Some(120_000)));
        assert!(!t.record_position(61_000, Some(120_000)));
    }

    #[test]
    fn long_tracks_qualify_after_four_minutes() {
        let mut t = ScrobbleTracker::default();
        t.start("a".into(), 0);
        assert!(!run(&mut t, 0, 239_000, 1_200_000));
        assert!(t.record_position(240_000, Some(1_200_000)));
    }

    #[test]
    fn short_or_unknown_length_tracks_never_qualify() {
        let mut t = ScrobbleTracker::default();
        t.start("a".into(), 0);
        assert!(!run(&mut t, 0, 20_000, 20_000));
        let mut u = ScrobbleTracker::default();
        u.start("a".into(), 0);
        assert!(!u.record_position(500_000, None));
    }

    #[test]
    fn seeking_forward_does_not_count_as_listening() {
        let mut t = ScrobbleTracker::default();
        t.start("a".into(), 0);
        t.record_position(1_000, Some(120_000));
        assert!(!t.record_position(100_000, Some(120_000)));
        assert_eq!(t.played_ms(), 1_000);
        t.rebase(100_000);
        t.record_position(101_000, Some(120_000));
        assert_eq!(t.played_ms(), 2_000);
        // Backwards jumps are ignored too.
        t.record_position(5_000, Some(120_000));
        assert_eq!(t.played_ms(), 2_000);
    }

    #[test]
    fn nothing_is_counted_before_start() {
        let mut t = ScrobbleTracker::default();
        assert!(!t.record_position(1_000, Some(120_000)));
    }
}
