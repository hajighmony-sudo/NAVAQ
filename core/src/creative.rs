//! Creative tools for the user: instrumental generation, stem separation
//! requests and Lyric-to-Melody generation.
//!
//! The core only models the requests and their lifecycle. Actual audio
//! synthesis / separation is performed by the host (local model, cloud AI,
//! or offline tool). Results are returned as ordinary `AudioSource` values
//! that can be enqueued like any other track.

use crate::composer::Composition;
use crate::model::{AudioSource, Track, TrackId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Unique id for a creative job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub String);

impl From<&str> for JobId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Status of a creative generation job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Request to turn lyrics into a full melody + harmony arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricToMelodyRequest {
    pub lyrics: String,
    /// Preferred style / genre (e.g. "lofi", "pop", "cinematic").
    pub style: Option<String>,
    /// Approximate tempo in BPM.
    pub tempo_bpm: Option<f32>,
    /// Key / mode hint ("C major", "A minor" …).
    pub key: Option<String>,
    /// Desired duration in seconds (host may ignore).
    pub duration_secs: Option<u32>,
    /// Whether to also generate an instrumental-only version.
    pub also_instrumental: bool,
}

/// Request to produce an instrumental (karaoke / backing track) version
/// of an existing track, or to separate stems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentalRequest {
    /// Source track that should be processed.
    pub source_track_id: TrackId,
    /// Which stems the user wants back.
    pub stems: Vec<StemKind>,
    /// If true, also produce a fully mixed instrumental (no vocals).
    pub produce_instrumental_mix: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    Vocals,
    Drums,
    Bass,
    Other,
    /// Full instrumental (everything except vocals).
    Instrumental,
}

/// A completed creative result ready to be turned into a `Track`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreativeResult {
    pub job_id: JobId,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: Option<u64>,
    pub source: AudioSource,
    /// Optional metadata describing how the result was produced.
    pub tags: BTreeMap<String, String>,
}

impl CreativeResult {
    /// Convert into a playable `Track` with a freshly supplied id.
    pub fn into_track(self, id: TrackId) -> Track {
        Track {
            id,
            title: self.title,
            artist: self.artist,
            album: Some("NAVAQ Creative".into()),
            duration_ms: self.duration_ms,
            source: self.source,
        }
    }
}

/// One creative job tracked by the core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreativeJob {
    pub id: JobId,
    pub status: JobStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub request: CreativeRequest,
    pub result: Option<CreativeResult>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreativeRequest {
    LyricToMelody(LyricToMelodyRequest),
    Instrumental(InstrumentalRequest),
    /// Render a hand-authored multi-instrument arrangement.
    ManualComposition(Composition),
}

/// Store of creative jobs (host drives the actual processing).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CreativeStore {
    jobs: BTreeMap<JobId, CreativeJob>,
}

impl CreativeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(
        &mut self,
        id: JobId,
        request: CreativeRequest,
        now_ms: u64,
    ) -> Result<&CreativeJob, CreativeError> {
        if self.jobs.contains_key(&id) {
            return Err(CreativeError::JobExists);
        }
        // Basic validation
        match &request {
            CreativeRequest::LyricToMelody(r) => {
                if r.lyrics.trim().is_empty() {
                    return Err(CreativeError::EmptyLyrics);
                }
                if r.lyrics.chars().count() > 20_000
                    || r.tempo_bpm
                        .is_some_and(|v| !v.is_finite() || !(30.0..=300.0).contains(&v))
                    || r.duration_secs.is_some_and(|v| !(1..=1800).contains(&v))
                    || r.style
                        .as_ref()
                        .is_some_and(|v| v.trim().is_empty() || v.len() > 80)
                    || r.key
                        .as_ref()
                        .is_some_and(|v| v.trim().is_empty() || v.len() > 32)
                {
                    return Err(CreativeError::InvalidRequest);
                }
            }
            CreativeRequest::Instrumental(r) => {
                if r.stems.is_empty() && !r.produce_instrumental_mix {
                    return Err(CreativeError::NoStemsRequested);
                }
                if r.source_track_id.0.trim().is_empty() {
                    return Err(CreativeError::InvalidRequest);
                }
                let mut unique = r.stems.clone();
                unique.sort_by_key(|stem| *stem as u8);
                unique.dedup();
                if unique.len() != r.stems.len() {
                    return Err(CreativeError::InvalidRequest);
                }
            }
            CreativeRequest::ManualComposition(composition) => {
                if !composition.is_valid() {
                    return Err(CreativeError::InvalidComposition);
                }
            }
        }

        let job = CreativeJob {
            id: id.clone(),
            status: JobStatus::Queued,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            request,
            result: None,
            error_message: None,
        };
        self.jobs.insert(id.clone(), job);
        Ok(self.jobs.get(&id).unwrap())
    }

    pub fn set_status(
        &mut self,
        id: &JobId,
        status: JobStatus,
        now_ms: u64,
    ) -> Result<(), CreativeError> {
        let job = self.jobs.get_mut(id).ok_or(CreativeError::JobNotFound)?;
        if status != JobStatus::Running || job.status != JobStatus::Queued {
            return Err(CreativeError::InvalidTransition);
        }
        if now_ms < job.updated_at_ms {
            return Err(CreativeError::TimeWentBackwards);
        }
        job.status = status;
        job.updated_at_ms = now_ms;
        Ok(())
    }

    pub fn complete(
        &mut self,
        id: &JobId,
        result: CreativeResult,
        now_ms: u64,
    ) -> Result<(), CreativeError> {
        let job = self.jobs.get_mut(id).ok_or(CreativeError::JobNotFound)?;
        if job.status != JobStatus::Running {
            return Err(CreativeError::InvalidTransition);
        }
        if result.title.trim().is_empty() || result.title.len() > 512
            || result.artist.as_ref().map_or(false, |artist| artist.len() > 256)
            || result.tags.len() > 64
            || result.tags.iter().any(|(key, value)| key.trim().is_empty() || key.len() > 64 || value.len() > 512)
        {
            return Err(CreativeError::InvalidRequest);
        }
        if &result.job_id != id {
            return Err(CreativeError::ResultJobMismatch);
        }
        if now_ms < job.updated_at_ms {
            return Err(CreativeError::TimeWentBackwards);
        }
        job.status = JobStatus::Succeeded;
        job.result = Some(result);
        job.error_message = None;
        job.updated_at_ms = now_ms;
        Ok(())
    }

    pub fn fail(&mut self, id: &JobId, message: String, now_ms: u64) -> Result<(), CreativeError> {
        let job = self.jobs.get_mut(id).ok_or(CreativeError::JobNotFound)?;
        if !matches!(job.status, JobStatus::Queued | JobStatus::Running) {
            return Err(CreativeError::InvalidTransition);
        }
        if now_ms < job.updated_at_ms {
            return Err(CreativeError::TimeWentBackwards);
        }
        job.status = JobStatus::Failed;
        job.result = None;
        job.error_message = Some(message);
        job.updated_at_ms = now_ms;
        Ok(())
    }

    pub fn cancel(&mut self, id: &JobId, now_ms: u64) -> Result<(), CreativeError> {
        let job = self.jobs.get_mut(id).ok_or(CreativeError::JobNotFound)?;
        if !matches!(job.status, JobStatus::Queued | JobStatus::Running) {
            return Err(CreativeError::AlreadyFinished);
        }
        if now_ms < job.updated_at_ms {
            return Err(CreativeError::TimeWentBackwards);
        }
        job.status = JobStatus::Cancelled;
        job.updated_at_ms = now_ms;
        Ok(())
    }

    pub fn get(&self, id: &JobId) -> Option<&CreativeJob> {
        self.jobs.get(id)
    }

    pub fn pending(&self) -> Vec<&CreativeJob> {
        self.jobs
            .values()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
            .collect()
    }

    pub fn remove(&mut self, id: &JobId) -> Option<CreativeJob> {
        self.jobs.remove(id)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CreativeError {
    #[error("job already exists")]
    JobExists,
    #[error("job not found")]
    JobNotFound,
    #[error("lyrics must not be empty")]
    EmptyLyrics,
    #[error("at least one stem or instrumental mix must be requested")]
    NoStemsRequested,
    #[error("job has already finished")]
    AlreadyFinished,
    #[error("manual composition is invalid")]
    InvalidComposition,
    #[error("creative request contains invalid options")]
    InvalidRequest,
    #[error("creative job state transition is not allowed")]
    InvalidTransition,
    #[error("result belongs to a different creative job")]
    ResultJobMismatch,
    #[error("job update timestamp cannot move backwards")]
    TimeWentBackwards,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyric_to_melody_job_lifecycle() {
        let mut store = CreativeStore::new();
        let req = CreativeRequest::LyricToMelody(LyricToMelodyRequest {
            lyrics: "Hello world under the stars".into(),
            style: Some("lofi".into()),
            tempo_bpm: Some(85.0),
            key: Some("C major".into()),
            duration_secs: Some(120),
            also_instrumental: true,
        });

        store.submit("job1".into(), req, 1000).unwrap();
        assert_eq!(store.get(&"job1".into()).unwrap().status, JobStatus::Queued);

        store
            .set_status(&"job1".into(), JobStatus::Running, 1100)
            .unwrap();

        let result = CreativeResult {
            job_id: "job1".into(),
            title: "Hello world under the stars".into(),
            artist: Some("NAVAQ AI".into()),
            duration_ms: Some(120_000),
            source: AudioSource::LocalFile("/generated/melody.wav".into()),
            tags: BTreeMap::from([("style".into(), "lofi".into())]),
        };
        store.complete(&"job1".into(), result, 5000).unwrap();

        let job = store.get(&"job1".into()).unwrap();
        assert_eq!(job.status, JobStatus::Succeeded);
        assert!(job.result.is_some());

        let track = job.result.clone().unwrap().into_track("gen-1".into());
        assert_eq!(track.title, "Hello world under the stars");
        assert_eq!(track.album.as_deref(), Some("NAVAQ Creative"));
    }

    #[test]
    fn creative_jobs_reject_invalid_transitions_and_wrong_result_ids() {
        let mut store = CreativeStore::new();
        let request = CreativeRequest::LyricToMelody(LyricToMelodyRequest {
            lyrics: "A short line".into(), style: None, tempo_bpm: None, key: None,
            duration_secs: None, also_instrumental: false,
        });
        store.submit("job".into(), request, 100).unwrap();
        let result = CreativeResult {
            job_id: "other".into(), title: "Generated".into(), artist: None,
            duration_ms: Some(1_000), source: AudioSource::LocalFile("/tmp/out.wav".into()),
            tags: BTreeMap::new(),
        };
        assert_eq!(store.complete(&"job".into(), result, 200), Err(CreativeError::InvalidTransition));
        store.set_status(&"job".into(), JobStatus::Running, 150).unwrap();
        let wrong = CreativeResult {
            job_id: "other".into(), title: "Generated".into(), artist: None,
            duration_ms: Some(1_000), source: AudioSource::LocalFile("/tmp/out.wav".into()),
            tags: BTreeMap::new(),
        };
        assert_eq!(store.complete(&"job".into(), wrong, 200), Err(CreativeError::ResultJobMismatch));
    }
    #[test]
    fn empty_lyrics_rejected() {
        let mut store = CreativeStore::new();
        let req = CreativeRequest::LyricToMelody(LyricToMelodyRequest {
            lyrics: "   ".into(),
            style: None,
            tempo_bpm: None,
            key: None,
            duration_secs: None,
            also_instrumental: false,
        });
        assert_eq!(
            store.submit("x".into(), req, 0),
            Err(CreativeError::EmptyLyrics)
        );
    }

    #[test]
    fn manual_composition_is_queued_for_rendering() {
        let mut store = CreativeStore::new();
        let composition = crate::composer::Composition {
            title: "Manual beat".into(),
            tempo_bpm: 100.0,
            numerator: 4,
            denominator: 4,
            key: "C minor".into(),
            bars: 4,
            tracks: vec![crate::composer::InstrumentTrack {
                id: "bass".into(),
                name: "Bass".into(),
                instrument: crate::composer::Instrument::Bass,
                volume: 0.8,
                pan: 0.0,
                muted: false,
                notes: vec![crate::composer::NoteEvent {
                    pitch: 36,
                    start_beat: 0.0,
                    duration_beats: 1.0,
                    velocity: 100,
                }],
            }],
        };
        store
            .submit(
                "manual".into(),
                CreativeRequest::ManualComposition(composition),
                10,
            )
            .unwrap();
        assert_eq!(
            store.get(&"manual".into()).unwrap().status,
            JobStatus::Queued
        );
    }

    #[test]
    fn instrumental_requires_stems_or_mix() {
        let mut store = CreativeStore::new();
        let req = CreativeRequest::Instrumental(InstrumentalRequest {
            source_track_id: "t1".into(),
            stems: vec![],
            produce_instrumental_mix: false,
        });
        assert_eq!(
            store.submit("x".into(), req, 0),
            Err(CreativeError::NoStemsRequested)
        );
    }
}
