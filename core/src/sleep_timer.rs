//! Deterministic sleep timer. The host feeds elapsed time; the core owns no clock.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepTimer {
    /// Stop playback after this much more playing time.
    After { remaining_ms: u64 },
    /// Stop when the current track finishes.
    EndOfTrack,
}

impl SleepTimer {
    pub fn after_ms(duration_ms: u64) -> Self {
        Self::After {
            remaining_ms: duration_ms,
        }
    }

    /// Advance a duration timer. Returns true once it has expired. Never fires for
    /// `EndOfTrack`, which is resolved by the runtime when the track ends.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        match self {
            Self::After { remaining_ms } => {
                *remaining_ms = remaining_ms.saturating_sub(elapsed_ms);
                *remaining_ms == 0
            }
            Self::EndOfTrack => false,
        }
    }

    pub fn remaining_ms(&self) -> Option<u64> {
        match self {
            Self::After { remaining_ms } => Some(*remaining_ms),
            Self::EndOfTrack => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_down_and_saturates() {
        let mut timer = SleepTimer::after_ms(1_000);
        assert!(!timer.tick(400));
        assert_eq!(timer.remaining_ms(), Some(600));
        assert!(timer.tick(10_000));
        assert_eq!(timer.remaining_ms(), Some(0));
    }

    #[test]
    fn end_of_track_never_expires_by_time() {
        let mut timer = SleepTimer::EndOfTrack;
        assert!(!timer.tick(u64::MAX));
        assert_eq!(timer.remaining_ms(), None);
    }
}
