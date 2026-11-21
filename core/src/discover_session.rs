//! Stateful, platform-neutral Discover search orchestration.
//! Provider I/O remains in the host; this module owns paging, result accumulation,
//! deterministic provider planning and bounded session history.

use crate::discover::{DiscoverError, DiscoverProvider, DiscoverSearchPage, DiscoverSearchRequest};
use crate::discover_catalog::DiscoverProviderRegistry;
use std::collections::BTreeSet;

const MAX_SESSION_RESULTS: usize = 500;
const MAX_PROVIDER_FAILURES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoverProviderSearch {
    pub provider: DiscoverProvider,
    pub request: DiscoverSearchRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoverProviderFailure {
    pub provider_id: String,
    pub message: String,
}

impl DiscoverProviderFailure {
    pub fn new(provider_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into().chars().take(128).collect(),
            message: message.into().chars().take(512).collect(),
        }
    }
}

/// Builds stable provider jobs without performing network requests. Disabled providers
/// are excluded, and providers without pagination are skipped after the first page.
pub fn plan_provider_searches(
    registry: &DiscoverProviderRegistry,
    request: &DiscoverSearchRequest,
) -> Result<Vec<DiscoverProviderSearch>, DiscoverError> {
    request.validate()?;
    Ok(registry
        .enabled()
        .filter(|provider| request.page == 0 || provider.supports_pagination)
        .map(|provider| DiscoverProviderSearch {
            provider: provider.clone(),
            request: request.clone(),
        })
        .collect())
}

/// A single search session. Pages must arrive in order; duplicate result URLs are
/// discarded while the first provider's metadata is retained.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoverSession {
    request: DiscoverSearchRequest,
    next_page: u32,
    has_more: bool,
    results: Vec<crate::discover::DiscoverResult>,
    seen_urls: BTreeSet<String>,
    failures: Vec<DiscoverProviderFailure>,
}

impl DiscoverSession {
    pub fn new(request: DiscoverSearchRequest) -> Result<Self, DiscoverError> {
        request.validate()?;
        let next_page = request.page;
        Ok(Self {
            request,
            next_page,
            has_more: true,
            results: Vec::new(),
            seen_urls: BTreeSet::new(),
            failures: Vec::new(),
        })
    }

    pub fn request(&self) -> &DiscoverSearchRequest {
        &self.request
    }

    pub fn results(&self) -> &[crate::discover::DiscoverResult] {
        &self.results
    }

    pub fn failures(&self) -> &[DiscoverProviderFailure] {
        &self.failures
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn next_request(&self) -> Option<DiscoverSearchRequest> {
        if !self.has_more {
            return None;
        }
        let mut request = self.request.clone();
        request.page = self.next_page;
        Some(request)
    }

    pub fn apply_page(&mut self, page: DiscoverSearchPage) -> Result<usize, DiscoverError> {
        page.validate()?;
        if page.query.trim() != self.request.query.trim() || page.page != self.next_page {
            return Err(DiscoverError::MismatchedPage);
        }
        if !self.has_more {
            return Err(DiscoverError::SearchExhausted);
        }

        let mut added = 0;
        for result in page.results {
            if self.results.len() >= MAX_SESSION_RESULTS {
                self.has_more = false;
                break;
            }
            if self.seen_urls.insert(result.url.clone()) {
                self.results.push(result);
                added += 1;
            }
        }
        self.has_more &= page.has_more && self.results.len() < MAX_SESSION_RESULTS;
        if self.has_more {
            self.next_page = self
                .next_page
                .checked_add(1)
                .ok_or(DiscoverError::InvalidPage)?;
        }
        Ok(added)
    }

    pub fn record_failure(&mut self, failure: DiscoverProviderFailure) {
        if self.failures.len() == MAX_PROVIDER_FAILURES {
            self.failures.remove(0);
        }
        self.failures.push(failure);
    }

    pub fn restart(&mut self, request: DiscoverSearchRequest) -> Result<(), DiscoverError> {
        request.validate()?;
        self.next_page = request.page;
        self.request = request;
        self.has_more = true;
        self.results.clear();
        self.seen_urls.clear();
        self.failures.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::{DiscoverProvider, DiscoverResult};

    fn result(id: &str, url: &str) -> DiscoverResult {
        DiscoverResult {
            id: id.into(),
            title: id.into(),
            url: url.into(),
            snippet: None,
            source_name: "provider".into(),
            media_type: Some("audio/mpeg".into()),
        }
    }

    fn page(index: u32, urls: &[&str], has_more: bool) -> DiscoverSearchPage {
        DiscoverSearchPage {
            query: "ambient".into(),
            page: index,
            results: urls
                .iter()
                .enumerate()
                .map(|(i, url)| result(&format!("r{i}"), url))
                .collect(),
            has_more,
        }
    }

    fn provider(id: &str, pagination: bool, enabled: bool) -> DiscoverProvider {
        DiscoverProvider {
            id: id.into(),
            display_name: id.into(),
            enabled,
            supports_pagination: pagination,
        }
    }

    #[test]
    fn session_accumulates_pages_and_deduplicates_urls() {
        let mut session =
            DiscoverSession::new(DiscoverSearchRequest::new("ambient").unwrap()).unwrap();
        assert_eq!(
            session.apply_page(page(0, &["https://cdn.example/a"], true)),
            Ok(1)
        );
        assert_eq!(session.next_request().unwrap().page, 1);
        assert_eq!(
            session.apply_page(page(
                1,
                &["https://cdn.example/a", "https://cdn.example/b"],
                false
            )),
            Ok(1)
        );
        assert_eq!(session.results().len(), 2);
        assert!(!session.has_more());
        assert!(session.next_request().is_none());
    }

    #[test]
    fn session_rejects_out_of_order_pages_and_restarts_cleanly() {
        let mut session = DiscoverSession::new(DiscoverSearchRequest::new("ambient").unwrap()).unwrap();
        assert_eq!(
            session.apply_page(page(1, &["https://cdn.example/a"], false)),
            Err(DiscoverError::MismatchedPage)
        );
        session
            .apply_page(page(0, &["https://cdn.example/a"], false))
            .unwrap();
        session
            .restart(DiscoverSearchRequest::new("jazz").unwrap())
            .unwrap();
        assert!(session.results().is_empty());
        assert_eq!(session.request().query, "jazz");
    }

    #[test]
    fn provider_plan_respects_enabled_and_pagination_capabilities() {
        let mut registry = DiscoverProviderRegistry::new();
        registry.register(provider("web", true, true)).unwrap();
        registry.register(provider("legacy", false, true)).unwrap();
        registry.register(provider("off", true, false)).unwrap();
        let mut request = DiscoverSearchRequest::new("ambient").unwrap();
        assert_eq!(
            plan_provider_searches(&registry, &request).unwrap().len(),
            2
        );
        request.page = 1;
        let plan = plan_provider_searches(&registry, &request).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].provider.id, "web");
    }

    #[test]
    fn failure_history_is_bounded() {
        let mut session = DiscoverSession::new(DiscoverSearchRequest::new("ambient").unwrap()).unwrap();
        for i in 0..(MAX_PROVIDER_FAILURES + 5) {
            session.record_failure(DiscoverProviderFailure::new("web", format!("error {i}")));
        }
        assert_eq!(session.failures().len(), MAX_PROVIDER_FAILURES);
        assert_eq!(session.failures()[0].message, "error 5");
    }
}
