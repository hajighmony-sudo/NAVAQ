//! Aggregate download queue operations and stable summary reporting.
//! The host still performs HTTP transfers and filesystem writes.

use crate::downloads::{DownloadError, DownloadId, DownloadManager, DownloadStatus};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DownloadSummary {
    pub total: usize,
    pub queued: usize,
    pub downloading: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub received_bytes: u64,
    pub expected_bytes: u64,
    pub tasks_with_known_size: usize,
}

impl DownloadSummary {
    pub fn active(&self) -> usize {
        self.queued + self.downloading + self.paused
    }

    pub fn progress_percent(&self) -> Option<f32> {
        if self.tasks_with_known_size == 0 || self.expected_bytes == 0 {
            return None;
        }
        Some(((self.received_bytes.min(self.expected_bytes) as f64
            / self.expected_bytes as f64) * 100.0) as f32)
    }
}

/// Convenience operations for controlling tasks by id without exposing mutable
/// task references to every platform adapter.
pub trait DownloadManagerExt {
    fn pause_download(&mut self, id: &DownloadId) -> Result<(), DownloadError>;
    fn resume_download(&mut self, id: &DownloadId) -> Result<(), DownloadError>;
    fn cancel_download(&mut self, id: &DownloadId) -> Result<(), DownloadError>;
    fn retry_download(&mut self, id: &DownloadId) -> Result<(), DownloadError>;
    fn summarize(&self) -> DownloadSummary;
}

impl DownloadManagerExt for DownloadManager {
    fn pause_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id).ok_or(DownloadError::TaskNotFound)?.pause()
    }

    fn resume_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id).ok_or(DownloadError::TaskNotFound)?.resume()
    }

    fn cancel_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id).ok_or(DownloadError::TaskNotFound)?.cancel()
    }

    fn retry_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id).ok_or(DownloadError::TaskNotFound)?.retry()
    }

    fn summarize(&self) -> DownloadSummary {
        let mut summary = DownloadSummary::default();
        for task in self.tasks() {
            summary.total += 1;
            summary.received_bytes = summary.received_bytes.saturating_add(task.received_bytes);
            if let Some(total) = task.expected_bytes {
                summary.expected_bytes = summary.expected_bytes.saturating_add(total);
                summary.tasks_with_known_size += 1;
            }
            match task.status {
                DownloadStatus::Queued => summary.queued += 1,
                DownloadStatus::Downloading => summary.downloading += 1,
                DownloadStatus::Paused => summary.paused += 1,
                DownloadStatus::Completed => summary.completed += 1,
                DownloadStatus::Failed => summary.failed += 1,
                DownloadStatus::Cancelled => summary.cancelled += 1,
            }
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::DownloadRequest;

    fn request(id: &str) -> DownloadRequest {
        DownloadRequest {
            id: id.into(),
            url: "https://cdn.example.org/audio.mp3".into(),
            file_name: None,
        }
    }

    #[test]
    fn manager_operations_and_summary_are_consistent() {
        let mut manager = DownloadManager::new();
        manager.enqueue(request("one")).unwrap();
        manager.enqueue(request("two")).unwrap();
        let id: DownloadId = "one".into();
        manager.get_mut(&id).unwrap().start().unwrap();
        manager.get_mut(&id).unwrap()
            .set_response_metadata(Some(200), Some("audio/mpeg".into()), true).unwrap();
        manager.get_mut(&id).unwrap().update_progress(50).unwrap();

        assert_eq!(manager.summarize().total, 2);
        assert_eq!(manager.summarize().downloading, 1);
        assert_eq!(manager.summarize().queued, 1);
        assert_eq!(manager.summarize().progress_percent(), Some(25.0));

        manager.pause_download(&id).unwrap();
        assert_eq!(manager.summarize().paused, 1);
        manager.resume_download(&id).unwrap();
        manager.cancel_download(&id).unwrap();
        assert_eq!(manager.summarize().cancelled, 1);
    }

    #[test]
    fn missing_task_returns_typed_error() {
        let mut manager = DownloadManager::new();
        assert_eq!(
            manager.pause_download(&"missing".into()),
            Err(DownloadError::TaskNotFound)
        );
    }
}
