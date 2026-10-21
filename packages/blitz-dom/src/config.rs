use crate::{HtmlParserProvider, UserAgentStylesheet};
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
/// `Parallel` traversals are serialised process-wide (Stylo's global pool can
/// only run one traversal at a time), so documents resolving from several
/// threads never run their style traversals concurrently. Use
/// [`StyleThreading::Sequential`] when many threads each resolve their own
/// documents and already saturate the CPU (e.g. a test runner).
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleThreading {
    /// Use Stylo's parallel traversal via its global rayon thread pool.
    /// Fastest for documents resolved from a single thread, including nested
    /// sub-documents.
    #[default]
    Parallel,
    /// Run style traversal sequentially on the calling thread, bypassing
    /// the global pool and its lock.
    Sequential,
}

/// Options used when constructing a [`BaseDocument`](crate::BaseDocument)
pub struct DocumentConfig {
    /// The initial `Viewport`
    pub viewport: Option<Viewport>,
    /// The base url which relative URLs are resolved against
    pub base_url: Option<String>,
    /// User Agent stylesheets, in cascade order. Defaults to `vec![UserAgentStylesheet::default_sheet()]`.
    ///
    /// The list is used as given, so replacing it removes the default sheet unless it is
    /// included, e.g. `vec![UserAgentStylesheet::default_sheet(), custom]`.
    pub ua_stylesheets: Vec<UserAgentStylesheet>,
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
    /// Whether incremental layout is enabled. Defaults to `true`.
    ///
    /// When disabled, every node is treated as damaged on each `resolve` and the
    /// whole tree is reconstructed and laid out from scratch through the same
    /// damage-driven pipeline used by incremental mode.
    pub incremental: Option<bool>,
    /// If set, every sub-resource `Request` blitz-dom creates for this
    /// document will carry this signal. Aborting it cancels every in-flight
    /// fetch tied to this document.
    pub abort_signal: Option<AbortSignal>,
    /// How deeply this document is nested within other documents
    /// (0 for a root document). Used to limit `<iframe>` nesting depth.
    pub subdocument_depth: usize,
}

impl Default for DocumentConfig {
    fn default() -> Self {
        Self {
            viewport: None,
            base_url: None,
            ua_stylesheets: vec![UserAgentStylesheet::default_sheet()],
            net_provider: None,
            navigation_provider: None,
            shell_provider: None,
            html_parser_provider: None,
            font_ctx: None,
            media_type: None,
            style_threading: StyleThreading::default(),
            incremental: None,
            abort_signal: None,
            subdocument_depth: 0,
        }
    }
}
