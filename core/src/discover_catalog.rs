//! Provider registry and deterministic result-page composition for Discover.

use crate::discover::{DiscoverError, DiscoverProvider, DiscoverResult, DiscoverSearchPage};
use std::collections::{BTreeMap, BTreeSet};

const MAX_MERGED_RESULTS: usize = 100;

/// Validated provider registry. Provider credentials and network clients remain host-owned.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoverProviderRegistry {
    providers: BTreeMap<String, DiscoverProvider>,
}

impl DiscoverProviderRegistry {
    pub fn new() -> Self { Self::default() }
    pub fn len(&self) -> usize { self.providers.len() }
    pub fn is_empty(&self) -> bool { self.providers.is_empty() }

    pub fn register(&mut self, provider: DiscoverProvider) -> Result<(), DiscoverError> {
        provider.validate()?;
        if self.providers.contains_key(&provider.id) {
            return Err(DiscoverError::DuplicateProvider);
        }
        self.providers.insert(provider.id.clone(), provider);
        Ok(())
    }

    pub fn upsert(&mut self, provider: DiscoverProvider) -> Result<(), DiscoverError> {
        provider.validate()?;
        self.providers.insert(provider.id.clone(), provider);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&DiscoverProvider> {
        self.providers.get(id)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), DiscoverError> {
        let provider = self.providers.get_mut(id).ok_or(DiscoverError::ProviderNotFound)?;
        provider.enabled = enabled;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<DiscoverProvider> {
        self.providers.remove(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &DiscoverProvider> {
        self.providers.values()
    }

    pub fn enabled(&self) -> impl Iterator<Item = &DiscoverProvider> {
        self.providers.values().filter(|provider| provider.enabled)
    }
}

/// Combines provider pages in input order, preserving the first result for each exact URL.
/// The caller should pass pages for the same query and page index.
pub fn merge_discover_pages(
    pages: &[DiscoverSearchPage],
) -> Result<DiscoverSearchPage, DiscoverError> {
    let first = pages.first().ok_or(DiscoverError::InvalidPage)?;
    first.validate()?;
    let query = first.query.trim().to_owned();
    let page = first.page;
    let mut has_more = false;
    let mut results: Vec<DiscoverResult> = Vec::new();
    let mut seen = BTreeSet::new();

    for source in pages {
        source.validate()?;
        if source.query.trim() != query || source.page != page {
            return Err(DiscoverError::MismatchedPage);
        }
        has_more |= source.has_more;
        for result in &source.results {
            if seen.insert(result.url.clone()) {
                if results.len() < MAX_MERGED_RESULTS {
                    results.push(result.clone());
                } else {
                    has_more = true;
                }
            }
        }
    }

    Ok(DiscoverSearchPage { query, page, results, has_more })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str) -> DiscoverProvider {
        DiscoverProvider {
            id: id.into(),
            display_name: format!("Provider {id}"),
            enabled: true,
            supports_pagination: true,
        }
    }

    fn page(query: &str, url: &str, id: &str) -> DiscoverSearchPage {
        DiscoverSearchPage {
            query: query.into(),
            page: 0,
            results: vec![DiscoverResult {
                id: id.into(),
                title: "Track".into(),
                url: url.into(),
                snippet: None,
                source_name: "Provider".into(),
                media_type: Some("audio/mpeg".into()),
            }],
            has_more: false,
        }
    }

    #[test]
    fn registry_validates_and_controls_providers() {
        let mut registry = DiscoverProviderRegistry::new();
        registry.register(provider("web")).unwrap();
        assert_eq!(registry.register(provider("web")), Err(DiscoverError::DuplicateProvider));
        registry.set_enabled("web", false).unwrap();
        assert_eq!(registry.enabled().count(), 0);
        assert!(registry.remove("web").is_some());
        assert!(registry.is_empty());
    }

    #[test]
    fn merged_pages_deduplicate_exact_urls_and_keep_provider_order() {
        let merged = merge_discover_pages(&[
            page("ambient", "https://example.org/a", "one"),
            page("ambient", "https://example.org/a", "duplicate"),
            page("ambient", "https://example.org/b", "two"),
        ]).unwrap();
        assert_eq!(merged.results.len(), 2);
        assert_eq!(merged.results[0].id, "one");
        assert_eq!(merged.results[1].id, "two");
    }

    #[test]
    fn merged_pages_reject_mismatched_queries() {
        assert_eq!(
            merge_discover_pages(&[page("one", "https://example.org/a", "a"),
                page("two", "https://example.org/b", "b")]),
            Err(DiscoverError::MismatchedPage)
        );
    }
}
