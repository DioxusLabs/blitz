use crate::HtmlParserProvider;
use blitz_traits::{
    navigation::NavigationProvider,
    net::NetProvider,
    shell::{ShellProvider, Viewport},
};
use parley::FontContext;
use std::sync::Arc;
use style::media_queries::MediaType;

/// Options used when constructing a [`BaseDocument`](crate::BaseDocument)
#[derive(Default)]
pub struct DocumentConfig {
    /// The initial `Viewport`
    pub viewport: Option<Viewport>,
    /// The base url which relative URLs are resolved against
    pub base_url: Option<String>,
    /// User Agent stylesheets
    pub ua_stylesheets: Option<Vec<String>>,
    /// Net provider to handle network requests for resources
    pub net_provider: Option<Arc<dyn NetProvider>>,
    /// Navigation provider to handle link clicks and form submissions
    pub navigation_provider: Option<Arc<dyn NavigationProvider>>,
    /// Shell provider to redraw requests, clipboard, etc
    pub shell_provider: Option<Arc<dyn ShellProvider>>,
    /// HTML parser provider. Used to parse HTML for setInnerHTML
    pub html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    /// Parley `FontContext`
    pub font_ctx: Option<FontContext>,
    /// The CSS media type used to evaluate `@media` rules.
    /// Defaults to [`MediaType::screen`].
    pub media_type: Option<MediaType>,

    /// Number of threads stylo uses for parallel style traversal.
    ///
    /// - `None` (default): blitz's historical default. The number of
    ///   threads is chosen automatically (`num_cpus * 3/4`, capped at 6).
    /// - `Some(1)`: disable parallel traversal. Stylo runs style work
    ///   serially on the calling thread.
    /// - `Some(n)` with `n >= 2`: use a pool of `n` worker threads.
    ///
    /// Set this to `Some(1)` when the embedder runs many small `Document`s
    /// concurrently on separate OS threads (e.g. one `Document` per request
    /// in a server, dispatched via `tokio::task::spawn_blocking`). Stylo's
    /// [`STYLE_THREAD_POOL`] is process-global; two parallel traversals
    /// landing on the same rayon worker will both try to borrow the
    /// worker's thread-local sharing cache mutably and one will panic.
    /// Pinning to a single thread keeps each render's stylo work on the
    /// caller's OS thread, where the thread-local is uniquely owned.
    ///
    /// # One-shot
    ///
    /// Stylo's [`STYLE_THREAD_POOL`] is a `LazyLock` initialised on first
    /// access. The value supplied here on the **first** `BaseDocument`
    /// constructed in the process wins; later `BaseDocument`s inherit that
    /// pool size regardless of what they pass.
    ///
    /// [`STYLE_THREAD_POOL`]: https://doc.servo.org/style/global_style_data/static.STYLE_THREAD_POOL.html
    pub stylo_thread_count: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards against a default-value drift on `stylo_thread_count`.
    /// `None` is load-bearing: it's how `Document::new` knows to fall back
    /// to the historical `-1` (auto) behaviour. A future refactor that
    /// accidentally changed the default to `Some(0)` or `Some(1)` would
    /// silently disable stylo parallelism for every existing caller.
    #[test]
    fn default_stylo_thread_count_is_none() {
        let config = DocumentConfig::default();
        assert!(config.stylo_thread_count.is_none());
    }
}
