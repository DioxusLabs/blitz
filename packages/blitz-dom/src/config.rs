use crate::{HtmlParserProvider, SharedImageCache};
use blitz_traits::{
    navigation::NavigationProvider,
    net::{AbortSignal, NetProvider},
    shell::{ShellProvider, Viewport},
};
use parley::FontContext;
use std::sync::Arc;
use style::media_queries::MediaType;

/// Strategy for Stylo's style traversal during `resolve`.
///
/// Two `Document`s resolving on [`StyleThreading::Parallel`] concurrently
/// share Stylo's global thread pool and can panic with
/// `already mutably borrowed` — see
/// <https://github.com/DioxusLabs/blitz/issues/430>. Set
/// [`StyleThreading::Sequential`] on documents that may resolve from a
/// user thread while another `Parallel` resolve is in flight.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleThreading {
    /// Use Stylo's parallel traversal via its global rayon thread pool.
    /// Fastest for a single document; panics if another `Parallel` resolve
    /// is in flight on a different thread.
    #[default]
    Parallel,
    /// Run style traversal sequentially on the calling thread, bypassing
    /// the global pool. Safe to use from many user threads concurrently.
    Sequential,
}

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
    /// Strategy for Stylo's style traversal.
    /// Defaults to [`StyleThreading::Parallel`].
    pub style_threading: StyleThreading,
    /// If set, every sub-resource `Request` blitz-dom creates for this
    /// document will carry this signal. Aborting it cancels every in-flight
    /// fetch tied to this document.
    pub abort_signal: Option<AbortSignal>,
    /// Optional shared in-memory image cache. When set, decoded images persist
    /// across documents that share this cache (e.g. across refreshes of the
    /// same tab), avoiding re-fetching on reload. If `None`, the document
    /// allocates its own private cache.
    pub image_cache: Option<SharedImageCache>,
}
