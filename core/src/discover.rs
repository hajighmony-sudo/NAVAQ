//! Platform-neutral discovery/search contracts. Provider networking and browser rendering
//! are host responsibilities; this module validates requests and normalizes provider results.

use crate::downloads::validate_http_url;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_QUERY_BYTES: usize = 512;
const MAX_RESULTS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverSearchRequest {
    pub query: String,
    pub page: u32,
    pub page_size: u16,
    pub safe_search: bool,
}

impl DiscoverSearchRequest {
    pub fn new(query: impl Into<String>) -> Result<Self, DiscoverError> {
        let request = Self { query: query.into(), page: 0, page_size: 20, safe_search: true };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), DiscoverError> {
        let query = self.query.trim();
        if query.is_empty() || query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control) {
            return Err(DiscoverError::InvalidQuery);
        }
        if self.page_size == 0 || usize::from(self.page_size) > MAX_RESULTS {
            return Err(DiscoverError::InvalidPageSize);
        }
        if self.page > 10_000 {
            return Err(DiscoverError::InvalidPage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverResult {
    pub id: String,
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
    pub source_name: String,
    pub media_type: Option<String>,
}

impl DiscoverResult {
    pub fn validate(&self) -> Result<(), DiscoverError> {
        if self.id.trim().is_empty() || self.id.len() > 256
            || self.title.trim().is_empty() || self.title.len() > 512
            || self.source_name.trim().is_empty() || self.source_name.len() > 128
            || self.snippet.as_ref().is_some_and(|s| s.len() > 4_096)
            || self.media_type.as_ref().is_some_and(|s| s.len() > 128)
        {
            return Err(DiscoverError::InvalidResult);
        }
        validate_http_url(&self.url).map_err(|_| DiscoverError::InvalidResult)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverSearchPage {
    pub query: String,
    pub page: u32,
    pub results: Vec<DiscoverResult>,
    pub has_more: bool,
}

impl DiscoverSearchPage {
    pub fn validate(&self) -> Result<(), DiscoverError> {
        if self.query.trim().is_empty() || self.query.len() > MAX_QUERY_BYTES
            || self.results.len() > MAX_RESULTS
        {
            return Err(DiscoverError::InvalidPage);
        }
        for result in &self.results {
            result.validate()?;
        }
        Ok(())
    }
}

/// Describes a provider; actual API calls and credentials remain outside the Rust core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverProvider {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub supports_pagination: bool,
}

impl DiscoverProvider {
    pub fn validate(&self) -> Result<(), DiscoverError> {
        if self.id.trim().is_empty() || self.id.len() > 128
            || self.display_name.trim().is_empty() || self.display_name.len() > 128
        {
            return Err(DiscoverError::InvalidProvider);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiscoverError {
    #[error("search query is invalid")]
    InvalidQuery,
    #[error("search page is invalid")]
    InvalidPage,
    #[error("search page size must be between 1 and 100")]
    InvalidPageSize,
    #[error("search result is invalid")]
    InvalidResult,
    #[error("search provider is invalid")]
    InvalidProvider,
    #[error("search provider is already registered")]
    DuplicateProvider,
    #[error("search provider was not found")]
    ProviderNotFound,
    #[error("search pages do not belong to the same query and page index")]
    MismatchedPage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_search_query_and_pagination() {
        let mut request = DiscoverSearchRequest::new("NAVAQ music").unwrap();
        assert_eq!(request.page_size, 20);
        request.page_size = 101;
        assert_eq!(request.validate(), Err(DiscoverError::InvalidPageSize));
        request.page_size = 10;
        request.query = "  ".into();
        assert_eq!(request.validate(), Err(DiscoverError::InvalidQuery));
    }

    #[test]
    fn validates_results_and_rejects_private_destinations() {
        let result = DiscoverResult {
            id: "r1".into(),
            title: "Track".into(),
            url: "https://example.org/track".into(),
            snippet: None,
            source_name: "Public Search".into(),
            media_type: Some("audio/mpeg".into()),
        };
        assert!(result.validate().is_ok());
        let mut unsafe_result = result;
        unsafe_result.url = "http://127.0.0.1/admin".into();
        assert_eq!(unsafe_result.validate(), Err(DiscoverError::InvalidResult));
    }
}
