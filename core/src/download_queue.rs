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
    pub known_size_received_bytes: u64,
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
        Some(
            ((self.known_size_received_bytes.min(self.expected_bytes) as f64
                / self.expected_bytes as f64)
                * 100.0) as f32,
        )
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

    /// Starts queued tasks until the configured number of concurrent transfers is reached.
    /// Returned ids are deterministic because the manager stores tasks in key order.
    fn start_available(&mut self, max_parallel: usize) -> Vec<DownloadId>;

    /// Pauses every currently downloading task and returns the number changed.
    fn pause_all(&mut self) -> usize;

    /// Cancels queued, downloading and paused tasks and returns the number changed.
    fn cancel_active(&mut self) -> usize;

    /// Resets all failed tasks for another attempt and returns the number changed.
    fn retry_failed(&mut self) -> usize;
}

impl DownloadManagerExt for DownloadManager {
    fn pause_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id).ok_or(DownloadError::TaskNotFound)?.pause()
    }

    fn resume_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id)
            .ok_or(DownloadError::TaskNotFound)?
            .resume()
    }

    fn cancel_download(&mut self, id: &DownloadId) -> Result<(), DownloadError> {
        self.get_mut(id)
            .ok_or(DownloadError::TaskNotFound)?
            .cancel()
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
                summary.known_size_received_bytes = summary
                    .known_size_received_bytes
                    .saturating_add(task.received_bytes.min(total));
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

    fn start_available(&mut self, max_parallel: usize) -> Vec<DownloadId> {
        let active = self
            .tasks()
            .filter(|task| task.status == DownloadStatus::Downloading)
            .count();
        let slots = max_parallel.saturating_sub(active);
        if slots == 0 {
            return Vec::new();
        }
        let queued: Vec<DownloadId> = self
            .tasks()
            .filter(|task| task.status == DownloadStatus::Queued)
            .take(slots)
            .map(|task| task.id.clone())
            .collect();
        let mut started = Vec::with_capacity(queued.len());
        for id in queued {
            if self.get_mut(&id).is_some_and(|task| task.start().is_ok()) {
                started.push(id);
            }
        }
        started
    }

    fn pause_all(&mut self) -> usize {
        let ids: Vec<DownloadId> = self
            .tasks()
            .filter(|task| task.status == DownloadStatus::Downloading)
            .map(|task| task.id.clone())
            .collect();
        let mut changed = 0;
        for id in ids {
            if self.get_mut(&id).is_some_and(|task| task.pause().is_ok()) {
                changed += 1;
            }
        }
        changed
    }

    fn cancel_active(&mut self) -> usize {
        let ids: Vec<DownloadId> = self
            .tasks()
            .filter(|task| {
                matches!(
                    task.status,
                    DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Paused
                )
            })
            .map(|task| task.id.clone())
            .collect();
        let mut changed = 0;
        for id in ids {
            if self.get_mut(&id).is_some_and(|task| task.cancel().is_ok()) {
                changed += 1;
            }
        }
        changed
    }

    fn retry_failed(&mut self) -> usize {
        let ids: Vec<DownloadId> = self
            .tasks()
            .filter(|task| task.status == DownloadStatus::Failed)
            .map(|task| task.id.clone())
            .collect();
        let mut changed = 0;
        for id in ids {
            if self.get_mut(&id).is_some_and(|task| task.retry().is_ok()) {
                changed += 1;
            }
        }
        changed
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
        manager
            .get_mut(&id)
            .unwrap()
            .set_response_metadata(Some(200), Some("audio/mpeg".into()), true)
            .unwrap();
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

    #[test]
    fn scheduler_respects_parallel_limit_and_starts_deterministically() {
        let mut manager = DownloadManager::new();
        manager.enqueue(request("c")).unwrap();
        manager.enqueue(request("a")).unwrap();
        manager.enqueue(request("b")).unwrap();
        let started = manager.start_available(2);
        assert_eq!(
            started,
            vec![DownloadId("a".into()), DownloadId("b".into())]
        );
        assert_eq!(manager.summarize().downloading, 2);
        assert_eq!(manager.summarize().queued, 1);
        assert!(manager.start_available(2).is_empty());
        assert_eq!(manager.start_available(3), vec![DownloadId("c".into())]);
    }

    #[test]
    fn bulk_controls_only_change_matching_states() {
        let mut manager = DownloadManager::new();
        manager.enqueue(request("one")).unwrap();
        manager.enqueue(request("two")).unwrap();
        manager.start_available(2);
        assert_eq!(manager.pause_all(), 2);
        assert_eq!(manager.summarize().paused, 2);
        assert_eq!(manager.cancel_active(), 2);
        assert_eq!(manager.summarize().cancelled, 2);
    }

    #[test]
    fn retry_failed_resets_only_failed_tasks() {
        let mut manager = DownloadManager::new();
        manager.enqueue(request("one")).unwrap();
        manager.get_mut(&"one".into()).unwrap().start().unwrap();
        manager
            .get_mut(&"one".into())
            .unwrap()
            .fail("offline")
            .unwrap();
        assert_eq!(manager.retry_failed(), 1);
        assert_eq!(manager.summarize().queued, 1);
        assert_eq!(manager.summarize().failed, 0);
    }
}
