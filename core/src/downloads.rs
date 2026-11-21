//! Platform-neutral download domain. Networking, filesystem access, notifications and
//! Android foreground-transfer policy belong to the platform host; this module owns
//! validation, task state transitions and deterministic progress accounting.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr};
use thiserror::Error;

const MAX_URL_BYTES: usize = 8_192;
const MAX_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub String);

impl From<&str> for DownloadId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub id: DownloadId,
    pub url: String,
    /// Optional user-selected name. The host must still create the file safely.
    pub file_name: Option<String>,
}

impl DownloadRequest {
    /// Creates a direct-download request from a validated Discover result.
    /// The host still performs the actual HTTP transfer and filesystem write.
    pub fn from_discover_result(
        id: impl Into<DownloadId>,
        result: &crate::discover::DiscoverResult,
        file_name: Option<String>,
    ) -> Result<Self, DownloadError> {
        let request = Self {
            id: id.into(),
            url: result.url.clone(),
            file_name,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), DownloadError> {
        if self.id.0.trim().is_empty() || self.id.0.len() > 128 {
            return Err(DownloadError::InvalidId);
        }
        validate_http_url(&self.url)?;
        if let Some(name) = &self.file_name {
            validate_file_name(name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    #[default]
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: DownloadId,
    pub url: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub expected_bytes: Option<u64>,
    pub received_bytes: u64,
    pub supports_range: bool,
    pub status: DownloadStatus,
    pub error: Option<String>,
}

impl DownloadTask {
    pub fn from_request(request: DownloadRequest) -> Result<Self, DownloadError> {
        request.validate()?;
        Ok(Self {
            id: request.id,
            url: request.url,
            file_name: request.file_name,
            mime_type: None,
            expected_bytes: None,
            received_bytes: 0,
            supports_range: false,
            status: DownloadStatus::Queued,
            error: None,
        })
    }

    pub fn progress_percent(&self) -> Option<f32> {
        let total = self.expected_bytes?;
        if total == 0 {
            return Some(100.0);
        }
        Some(((self.received_bytes.min(total) as f64 / total as f64) * 100.0) as f32)
    }

    pub fn start(&mut self) -> Result<(), DownloadError> {
        match self.status {
            DownloadStatus::Queued => {
                self.status = DownloadStatus::Downloading;
                self.error = None;
                Ok(())
            }
            DownloadStatus::Paused => self.resume(),
            _ => Err(DownloadError::InvalidTransition),
        }
    }

    pub fn pause(&mut self) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Downloading {
            return Err(DownloadError::InvalidTransition);
        }
        self.status = DownloadStatus::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Paused {
            return Err(DownloadError::InvalidTransition);
        }
        if self.received_bytes > 0 && !self.supports_range {
            return Err(DownloadError::RangeNotSupported);
        }
        self.status = DownloadStatus::Downloading;
        self.error = None;
        Ok(())
    }

    /// Called by the host after inspecting response headers (Content-Length, MIME and
    /// Accept-Ranges). It does not perform a network request.
    pub fn set_response_metadata(
        &mut self,
        expected_bytes: Option<u64>,
        mime_type: Option<String>,
        supports_range: bool,
    ) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Downloading {
            return Err(DownloadError::InvalidTransition);
        }
        if mime_type
            .as_ref()
            .is_some_and(|v| v.len() > 255 || v.chars().any(char::is_control))
        {
            return Err(DownloadError::InvalidMetadata);
        }
        if expected_bytes.is_some_and(|total| self.received_bytes > total) {
            return Err(DownloadError::InvalidProgress);
        }
        self.expected_bytes = expected_bytes;
        self.mime_type = mime_type;
        self.supports_range = supports_range;
        Ok(())
    }

    /// Progress must be monotonic for a single attempt and cannot exceed a known length.
    pub fn update_progress(&mut self, received_bytes: u64) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Downloading {
            return Err(DownloadError::InvalidTransition);
        }
        if received_bytes < self.received_bytes
            || self
                .expected_bytes
                .is_some_and(|total| received_bytes > total)
        {
            return Err(DownloadError::InvalidProgress);
        }
        self.received_bytes = received_bytes;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Downloading {
            return Err(DownloadError::InvalidTransition);
        }
        if self
            .expected_bytes
            .is_some_and(|total| self.received_bytes != total)
        {
            return Err(DownloadError::IncompleteDownload);
        }
        self.status = DownloadStatus::Completed;
        self.error = None;
        Ok(())
    }

    pub fn fail(&mut self, message: impl Into<String>) -> Result<(), DownloadError> {
        if !matches!(
            self.status,
            DownloadStatus::Downloading | DownloadStatus::Queued | DownloadStatus::Paused
        ) {
            return Err(DownloadError::InvalidTransition);
        }
        let message = message.into();
        self.error = Some(message.chars().take(512).collect());
        self.status = DownloadStatus::Failed;
        Ok(())
    }

    pub fn retry(&mut self) -> Result<(), DownloadError> {
        if self.status != DownloadStatus::Failed {
            return Err(DownloadError::InvalidTransition);
        }
        self.received_bytes = 0;
        self.expected_bytes = None;
        self.mime_type = None;
        self.supports_range = false;
        self.error = None;
        self.status = DownloadStatus::Queued;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), DownloadError> {
        if matches!(
            self.status,
            DownloadStatus::Completed | DownloadStatus::Cancelled
        ) {
            return Err(DownloadError::InvalidTransition);
        }
        self.status = DownloadStatus::Cancelled;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadManager {
    tasks: BTreeMap<DownloadId, DownloadTask>,
}

impl DownloadManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.tasks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    pub fn get(&self, id: &DownloadId) -> Option<&DownloadTask> {
        self.tasks.get(id)
    }
    pub fn tasks(&self) -> impl Iterator<Item = &DownloadTask> {
        self.tasks.values()
    }

    pub fn enqueue(&mut self, request: DownloadRequest) -> Result<&DownloadTask, DownloadError> {
        let task = DownloadTask::from_request(request)?;
        if self.tasks.contains_key(&task.id) {
            return Err(DownloadError::DuplicateId);
        }
        let id = task.id.clone();
        self.tasks.insert(id.clone(), task);
        Ok(self.tasks.get(&id).expect("inserted task"))
    }

    pub fn get_mut(&mut self, id: &DownloadId) -> Option<&mut DownloadTask> {
        self.tasks.get_mut(id)
    }

    pub fn remove(&mut self, id: &DownloadId) -> Option<DownloadTask> {
        self.tasks.remove(id)
    }

    pub fn clear_finished(&mut self) {
        self.tasks.retain(|_, task| {
            !matches!(
                task.status,
                DownloadStatus::Completed | DownloadStatus::Cancelled
            )
        });
    }
}

/// Validate a direct HTTP(S) resource URL before handing it to a platform downloader.
/// Private, loopback and local-network destinations are rejected to reduce SSRF risk.
/// Hosts must additionally validate DNS results and every redirect before connecting.
pub fn validate_http_url(url: &str) -> Result<(), DownloadError> {
    if url.is_empty() || url.len() > MAX_URL_BYTES || url.chars().any(char::is_control) {
        return Err(DownloadError::InvalidUrl);
    }
    let (scheme, remainder) = url.split_once("://").ok_or(DownloadError::InvalidUrl)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(DownloadError::UnsupportedScheme);
    }
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || authority.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(DownloadError::InvalidUrl);
    }
    let host = if authority.starts_with('[') {
        let end = authority.find(']').ok_or(DownloadError::InvalidUrl)?;
        let parsed: Ipv6Addr = authority[1..end]
            .parse()
            .map_err(|_| DownloadError::InvalidUrl)?;
        if authority[end + 1..].starts_with(':') {
            let port = &authority[end + 2..];
            if !port.is_empty() && port.parse::<u16>().is_err() {
                return Err(DownloadError::InvalidUrl);
            }
        } else if !authority[end + 1..].is_empty() {
            return Err(DownloadError::InvalidUrl);
        }
        if is_blocked_ip(IpAddr::V6(parsed)) {
            return Err(DownloadError::BlockedDestination);
        }
        return Ok(());
    } else {
        authority
            .rsplit_once(':')
            .filter(|(h, p)| !h.contains(':') && !p.is_empty())
            .map(|(h, p)| {
                if p.parse::<u16>().is_err() {
                    Err(DownloadError::InvalidUrl)
                } else {
                    Ok(h)
                }
            })
            .transpose()?
            .unwrap_or(authority)
    };
    if host.is_empty() || host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return Err(DownloadError::InvalidUrl);
    }
    if host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host.to_ascii_lowercase().ends_with(".local")
        || host.to_ascii_lowercase().ends_with(".internal")
    {
        return Err(DownloadError::BlockedDestination);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(DownloadError::BlockedDestination);
        }
    } else if host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || host.strip_prefix("0x").is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())
        })
    {
        // Reject non-canonical numeric IPv4 forms (for example 127.1 or 0x7f000001).
        return Err(DownloadError::BlockedDestination);
    } else if !host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    }) {
        return Err(DownloadError::InvalidUrl);
    }
    Ok(())
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6.to_ipv4().is_some_and(|v4| is_blocked_ip(IpAddr::V4(v4)))
        }
    }
}

fn validate_file_name(name: &str) -> Result<(), DownloadError> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(DownloadError::InvalidFileName);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error("download id is invalid")]
    InvalidId,
    #[error("download URL is invalid")]
    InvalidUrl,
    #[error("only HTTP and HTTPS direct links are supported")]
    UnsupportedScheme,
    #[error("local or private network destinations are not allowed")]
    BlockedDestination,
    #[error("download file name is invalid")]
    InvalidFileName,
    #[error("download metadata is invalid")]
    InvalidMetadata,
    #[error("download progress is invalid")]
    InvalidProgress,
    #[error("download is incomplete")]
    IncompleteDownload,
    #[error("server does not support resuming this partial download")]
    RangeNotSupported,
    #[error("download state transition is not allowed")]
    InvalidTransition,
    #[error("a download with this id already exists")]
    DuplicateId,
    #[error("download task was not found")]
    TaskNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str) -> DownloadRequest {
        DownloadRequest {
            id: id.into(),
            url: "https://cdn.example.org/audio/song.mp3".into(),
            file_name: Some("song.mp3".into()),
        }
    }

    #[test]
    fn discover_results_can_be_queued_as_direct_downloads() {
        let result = crate::discover::DiscoverResult {
            id: "provider-track-1".into(),
            title: "Track".into(),
            url: "https://cdn.example.org/track.mp3".into(),
            snippet: None,
            source_name: "Search".into(),
            media_type: Some("audio/mpeg".into()),
        };
        let request = DownloadRequest::from_discover_result(
            "download-1",
            &result,
            Some("track.mp3".into()),
        )
        .unwrap();
        assert_eq!(request.url, result.url);
        assert_eq!(request.file_name.as_deref(), Some("track.mp3"));

        let mut unsafe_result = result;
        unsafe_result.url = "http://127.0.0.1/private".into();
        assert_eq!(
            DownloadRequest::from_discover_result("bad", &unsafe_result, None),
            Err(DownloadError::BlockedDestination)
        );
    }

    #[test]
    fn validates_http_links_and_rejects_unsafe_destinations() {
        assert!(validate_http_url("https://example.com/file.mp3").is_ok());
        assert!(validate_http_url("http://192.168.1.2/file").is_err());
        assert!(validate_http_url("http://localhost/file").is_err());
        assert!(validate_http_url("file:///etc/passwd").is_err());
        assert!(validate_http_url("https://user:pass@example.com/a").is_err());
        assert!(validate_http_url("https://[::1]/a").is_err());
    }

    #[test]
    fn tracks_progress_pause_resume_and_completion() {
        let mut task = DownloadTask::from_request(request("one")).unwrap();
        task.start().unwrap();
        task.set_response_metadata(Some(100), Some("audio/mpeg".into()), true)
            .unwrap();
        task.update_progress(40).unwrap();
        assert_eq!(task.progress_percent(), Some(40.0));
        task.pause().unwrap();
        task.resume().unwrap();
        task.update_progress(100).unwrap();
        task.complete().unwrap();
        assert_eq!(task.status, DownloadStatus::Completed);
    }

    #[test]
    fn partial_download_without_range_support_cannot_resume() {
        let mut task = DownloadTask::from_request(request("one")).unwrap();
        task.start().unwrap();
        task.set_response_metadata(Some(100), None, false).unwrap();
        task.update_progress(20).unwrap();
        task.pause().unwrap();
        assert_eq!(task.resume(), Err(DownloadError::RangeNotSupported));
    }

    #[test]
    fn retry_resets_failed_attempt_and_manager_rejects_duplicate_ids() {
        let mut manager = DownloadManager::new();
        manager.enqueue(request("one")).unwrap();
        assert_eq!(
            manager.enqueue(request("one")).unwrap_err(),
            DownloadError::DuplicateId
        );
        let task = manager.get_mut(&"one".into()).unwrap();
        task.start().unwrap();
        task.fail("network").unwrap();
        task.retry().unwrap();
        assert_eq!(task.status, DownloadStatus::Queued);
        assert_eq!(task.received_bytes, 0);
    }

    #[test]
    fn rejects_path_traversal_file_names_and_non_monotonic_progress() {
        let mut bad = request("bad");
        bad.file_name = Some("../secret".into());
        assert_eq!(
            DownloadTask::from_request(bad).unwrap_err(),
            DownloadError::InvalidFileName
        );
        let mut task = DownloadTask::from_request(request("ok")).unwrap();
        task.start().unwrap();
        task.set_response_metadata(Some(50), None, true).unwrap();
        task.update_progress(20).unwrap();
        assert_eq!(
            task.update_progress(10),
            Err(DownloadError::InvalidProgress)
        );
    }
}
