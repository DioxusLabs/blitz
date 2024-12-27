/// A provider to enable a document to bubble up navigation events (e.g. clicking a link)
pub trait NavigationProvider: Send + Sync + 'static {
    fn navigate_new_page(&self, url: String);
}

pub struct DummyNavigationProvider;

impl NavigationProvider for DummyNavigationProvider {
    fn navigate_new_page(&self, _url: String) {
        // Default impl: do nothing
    }
}
