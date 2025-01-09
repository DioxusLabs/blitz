use url::Url;

/// A provider to enable a document to bubble up navigation events (e.g. clicking a link)
pub trait NavigationProvider: Send + Sync + 'static {
    fn navigate_new_page(&self, navigation: NavigationOptions);
}

pub struct DummyNavigationProvider;

impl NavigationProvider for DummyNavigationProvider {
    fn navigate_new_page(&self, _navigation: NavigationOptions) {
        // Default impl: do nothing
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct NavigationOptions {
    /// The URL to navigate to
    pub url: Url,

    /// Source document for the navigation
    pub source_document: usize,

    pub referrer_policy: String,
}

impl Default for NavigationOptions {
    fn default() -> Self {
        Self {
            url: Url::parse("http://localhost").unwrap(),
            source_document: 0,
            referrer_policy: String::new(),
        }
    }
}

impl NavigationOptions {
    pub fn new(url: Url, source_document: usize) -> Self {
        Self {
            url,
            source_document,
            ..Default::default()
        }
    }
    pub fn with_referrer_policy(mut self, resource: String) -> Self {
        self.referrer_policy = resource;
        self
    }
}
