//! [`ScriptDocument`]: a [`Document`] implementation with JavaScript support

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Waker};

use blitz_dom::{
    BaseDocument, DEFAULT_CSS, DocGuard, DocGuardMut, Document, DocumentConfig, EventDriver,
};
use blitz_html::{DocumentHtmlParser, HtmlProvider};
use blitz_traits::events::{DomEvent, UiEvent};
use url::Url;
use web_time::Instant;

use crate::event_handler::ScriptEventHandler;
use crate::fetch::{DefaultScriptFetcher, ScriptFetcher};
use crate::runtime::ScriptRuntime;

/// A `<script>` element found in the document
struct PendingScript {
    src: Option<String>,
    inline_text: String,
    is_module: bool,
}

/// A [`Document`] which executes the JavaScript contained in the document's
/// `<script>` tags, exposing DOM APIs backed by `blitz-dom` to the scripts.
///
/// Construct with [`ScriptDocument::from_html`], then call
/// [`execute_scripts`](ScriptDocument::execute_scripts) (this also happens
/// automatically on the first [`poll`](Document::poll)). UI events pushed via
/// [`handle_ui_event`](Document::handle_ui_event) are dispatched to JavaScript
/// event listeners before Blitz's default actions run.
pub struct ScriptDocument {
    inner: Rc<RefCell<BaseDocument>>,
    runtime: ScriptRuntime,
    base_url: Option<Url>,
    /// Shared with the module loader registered on the Boa context
    fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    scripts_executed: bool,

    // Timer wakeups: a background thread which wakes the event loop (via the
    // `Waker` passed to `poll`) when the next JS timer is due.
    waker: Arc<Mutex<Option<Waker>>>,
    timer_thread: Option<Sender<Instant>>,
    timer_thread_enabled: bool,
}

impl ScriptDocument {
    /// Parse HTML into a [`ScriptDocument`].
    ///
    /// Note: this does *not* execute any scripts yet. Call
    /// [`execute_scripts`](Self::execute_scripts) to do so (or rely on the
    /// first `poll` doing it automatically).
    pub fn from_html(html: &str, mut config: DocumentConfig) -> Self {
        if let Some(ss) = &mut config.ua_stylesheets {
            if !ss.iter().any(|s| s == DEFAULT_CSS) {
                ss.push(String::from(DEFAULT_CSS));
            }
        } else {
            config.ua_stylesheets = Some(vec![String::from(DEFAULT_CSS)]);
        }
        if config.html_parser_provider.is_none() {
            config.html_parser_provider = Some(Arc::new(HtmlProvider));
        }

        let mut doc = BaseDocument::new(config);
        let mut mutr = doc.mutate();
        DocumentHtmlParser::parse_into_mutator(&mut mutr, html);
        drop(mutr);

        Self::from_base_document(doc)
    }

    /// Wrap an already-parsed [`BaseDocument`] in a [`ScriptDocument`] without
    /// reparsing the HTML.
    ///
    /// The base URL for resolving external script sources is taken from the
    /// document ([`BaseDocument::base_url`]).
    ///
    /// Note: for `innerHTML` support the document must have been created with
    /// an HTML parser provider (e.g. `blitz_html::HtmlProvider`) set in its
    /// [`DocumentConfig`].
    pub fn from_base_document(doc: BaseDocument) -> Self {
        // The default base url (set when `DocumentConfig.base_url` is `None`) is a
        // meaningless data url. Treat it as "no base url".
        let base_url = Some(doc.base_url().clone()).filter(|url| url.scheme() != "data");

        let inner = Rc::new(RefCell::new(doc));
        let fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>> =
            Rc::new(RefCell::new(Box::new(DefaultScriptFetcher)));
        let runtime = ScriptRuntime::new(Rc::clone(&inner), base_url.as_ref(), Rc::clone(&fetcher));

        Self {
            inner,
            runtime,
            base_url,
            fetcher,
            scripts_executed: false,
            waker: Arc::new(Mutex::new(None)),
            timer_thread: None,
            timer_thread_enabled: true,
        }
    }

    /// Disable the background timer thread which wakes the event loop (via the
    /// `Waker` passed to `poll`) when the next JS timer is due.
    ///
    /// Useful for embedders which drive timers manually by polling
    /// [`next_timer_deadline`](Self::next_timer_deadline) and calling
    /// [`poll`](blitz_traits::Document::poll), and therefore don't need wakeups.
    pub fn without_timer_thread(mut self) -> Self {
        self.timer_thread_enabled = false;
        self
    }

    /// Switch the document's clock (which drives JS timer deadlines and
    /// `Date`) to virtual time: time stops and only advances via
    /// [`advance_clock_to`](Self::advance_clock_to).
    ///
    /// Intended for embedders which drive timers manually by polling
    /// [`next_timer_deadline`](Self::next_timer_deadline) and calling
    /// [`poll`](blitz_traits::Document::poll): instead of sleeping until the
    /// next deadline they can jump the clock straight to it, preserving timer
    /// ordering without wall-clock waiting. Should be combined with
    /// [`without_timer_thread`](Self::without_timer_thread) (the timer thread
    /// sleeps in real time).
    pub fn with_virtual_time(self) -> Self {
        self.runtime.ctx.state.borrow().clock.make_virtual();
        self
    }

    /// The current time according to the document's clock (virtual or real)
    pub fn clock_now(&self) -> Instant {
        self.runtime.ctx.state.borrow().clock.now()
    }

    /// Advance a virtual clock to `deadline` (never backwards). Does nothing
    /// unless [`with_virtual_time`](Self::with_virtual_time) was used.
    pub fn advance_clock_to(&mut self, deadline: Instant) {
        self.runtime.ctx.state.borrow().clock.advance_to(deadline);
    }

    /// Override the [`ScriptFetcher`] used to load external (`src="..."`) scripts
    /// and ES module imports. The default fetcher supports `file:` and `data:` URLs.
    pub fn with_fetcher(self, fetcher: impl ScriptFetcher) -> Self {
        *self.fetcher.borrow_mut() = Box::new(fetcher);
        self
    }

    /// Execute the document's `<script>` elements in document order, then fire
    /// the `DOMContentLoaded` and `load` events.
    ///
    /// Does nothing if scripts have already been executed.
    pub fn execute_scripts(&mut self) {
        if self.scripts_executed {
            return;
        }
        self.scripts_executed = true;

        for script in self.collect_scripts() {
            // HTML named element access: elements with ids are exposed as globals
            self.runtime.sync_named_element_globals();
            let (code, url) = match script.src {
                Some(src) => {
                    let Some(url) = self.resolve_script_url(&src) else {
                        self.record_error(format!("could not resolve script URL {src:?}"));
                        continue;
                    };
                    let fetch_result = self.fetcher.borrow().fetch(&url);
                    let code = match fetch_result {
                        Ok(code) => code,
                        Err(error) => {
                            self.record_error(format!("failed to fetch script {url}: {error}"));
                            continue;
                        }
                    };
                    (code, Some(url))
                }
                None => (script.inline_text, None),
            };

            if script.is_module {
                // Inline modules resolve imports against the document base URL
                let module_url = url.or_else(|| self.base_url.clone());
                self.runtime.eval_module(&code, module_url.as_ref());
            } else {
                let description = url
                    .as_ref()
                    .map(Url::as_str)
                    .unwrap_or("<inline script>")
                    .to_string();
                self.runtime.eval(&code, &description);
            }
        }

        self.runtime
            .set_ready_state(crate::state::ReadyState::Interactive);
        self.runtime.dispatch_document_event("DOMContentLoaded");
        self.runtime
            .set_ready_state(crate::state::ReadyState::Complete);
        self.runtime.install_body_onload_attribute();
        self.runtime.dispatch_window_event("load");
        self.runtime.ctx.flush_wrapper_switches();

        self.request_redraw();
        self.arm_timer_thread();
    }

    /// The resolved URLs of the document's external (`<script src="...">`) scripts,
    /// in document order.
    ///
    /// The [`ScriptFetcher`] API is synchronous, so embedders with asynchronous
    /// networking can use this to prefetch script sources before calling
    /// [`execute_scripts`](Self::execute_scripts), and then serve them from memory
    /// via a custom fetcher (see [`with_fetcher`](Self::with_fetcher)).
    pub fn external_script_urls(&self) -> Vec<Url> {
        self.collect_scripts()
            .iter()
            .filter_map(|script| script.src.as_deref())
            .filter_map(|src| self.resolve_script_url(src))
            .collect()
    }

    /// Resolve a script `src` attribute against the document's base URL
    fn resolve_script_url(&self, src: &str) -> Option<Url> {
        match &self.base_url {
            Some(base) => base.join(src).ok(),
            None => Url::parse(src).ok(),
        }
    }

    /// Evaluate arbitrary JavaScript code in the document's script context
    pub fn eval(&mut self, code: &str) {
        self.runtime.sync_named_element_globals();
        self.runtime.eval(code, "<eval>");
        self.runtime.ctx.flush_wrapper_switches();
        self.request_redraw();
        self.arm_timer_thread();
    }

    /// Drain messages sent from JavaScript via the global
    /// `__blitz_send_message(message)` native function.
    ///
    /// This provides a simple JS -> embedder communication channel (used, for
    /// example, by the WPT runner to collect testharness.js results).
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.runtime.ctx.state.borrow_mut().outbound_messages)
    }

    /// Drain uncaught JavaScript errors (from script loading/evaluation, event
    /// listeners, timer callbacks and promise jobs).
    ///
    /// Errors are captured rather than printed: it is up to the embedder to
    /// drain them and decide how to surface them (log, record as test
    /// failures, ...). At most 256 errors are retained between drains. When the
    /// `tracing` feature is enabled errors are additionally logged via
    /// [`tracing::error!`].
    pub fn take_js_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.runtime.ctx.state.borrow_mut().uncaught_errors)
    }

    /// Force a full garbage collection of the JS heap.
    pub fn run_gc(&mut self) {
        boa_gc::force_collect();
    }

    /// Record an error for collection via [`take_js_errors`](Self::take_js_errors)
    fn record_error(&mut self, message: String) {
        #[cfg(feature = "tracing")]
        tracing::error!("blitz-vibey-script: {message}");
        self.runtime.ctx.state.borrow_mut().record_error(message);
    }

    /// The deadline of the soonest pending JS timer (if any).
    ///
    /// Embedders which drive the document manually (rather than through an
    /// event loop `Waker`) can sleep until this deadline and then call
    /// [`poll`](Document::poll) to run due timers.
    pub fn next_timer_deadline(&self) -> Option<Instant> {
        self.runtime.next_timer_deadline()
    }

    /// Dispatch a synthetic DOM event (e.g. a click created with
    /// [`Node::synthetic_click_event`](blitz_dom::Node::synthetic_click_event))
    /// through the document's event driver. The event is exposed to JavaScript
    /// event listeners, and Blitz's default actions run unless prevented.
    pub fn dispatch_dom_event(&mut self, event: DomEvent) {
        let handler = ScriptEventHandler {
            runtime: &mut self.runtime,
        };
        let mut driver = EventDriver::new(&mut self.inner, handler);
        driver.handle_dom_event(event);
        self.runtime.ctx.flush_wrapper_switches();

        self.request_redraw();
        self.arm_timer_thread();
    }

    /// Find `<script>` elements in document order
    fn collect_scripts(&self) -> Vec<PendingScript> {
        let doc = self.inner.borrow();
        let mut scripts = Vec::new();
        let mut stack = vec![doc.root_node().id];

        while let Some(node_id) = stack.pop() {
            let Some(node) = doc.get_node(node_id) else {
                continue;
            };

            if let Some(element) = node.element_data() {
                if element.name.local == blitz_dom::local_name!("script") {
                    // Skip non-JavaScript script types (e.g. JSON data blocks)
                    let script_type = element
                        .attr(blitz_dom::local_name!("type"))
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();
                    let is_js = matches!(
                        script_type.as_str(),
                        "" | "text/javascript" | "application/javascript" | "module"
                    );
                    if is_js {
                        scripts.push(PendingScript {
                            src: element
                                .attr(blitz_dom::local_name!("src"))
                                .map(str::to_string),
                            inline_text: node.text_content(),
                            is_module: script_type == "module",
                        });
                    }
                    continue;
                }
            }

            stack.extend(node.children.iter().rev().copied());
        }

        scripts
    }

    fn request_redraw(&self) {
        self.inner.borrow().shell_provider.request_redraw();
    }

    /// Ensure the timer thread is armed to wake the event loop when the next
    /// JS timer is due.
    fn arm_timer_thread(&mut self) {
        if !self.timer_thread_enabled {
            return;
        }
        let Some(deadline) = self.runtime.next_timer_deadline() else {
            return;
        };

        let sender = self.timer_thread.get_or_insert_with(|| {
            let (tx, rx) = channel::<Instant>();
            let waker = Arc::clone(&self.waker);
            std::thread::Builder::new()
                .name("blitz-vibey-script-timers".to_string())
                .spawn(move || timer_thread_main(rx, waker))
                .expect("failed to spawn timer thread");
            tx
        });

        // If the thread has exited (channel disconnected) drop the sender so a
        // new thread is spawned next time.
        if sender.send(deadline).is_err() {
            self.timer_thread = None;
        }
    }
}

/// Background thread which wakes the event loop when JS timers are due
fn timer_thread_main(rx: Receiver<Instant>, waker: Arc<Mutex<Option<Waker>>>) {
    let mut deadline: Option<Instant> = None;

    loop {
        match deadline {
            None => match rx.recv() {
                Ok(new_deadline) => deadline = Some(new_deadline),
                Err(_) => return,
            },
            Some(current) => {
                let now = Instant::now();
                if current <= now {
                    if let Some(waker) = waker.lock().unwrap().as_ref() {
                        waker.wake_by_ref();
                    }
                    deadline = None;
                    continue;
                }
                match rx.recv_timeout(current - now) {
                    Ok(new_deadline) => deadline = Some(new_deadline.min(current)),
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(waker) = waker.lock().unwrap().as_ref() {
                            waker.wake_by_ref();
                        }
                        deadline = None;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

impl Document for ScriptDocument {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::RefCell(self.inner.borrow())
    }

    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::RefCell(self.inner.borrow_mut())
    }

    fn handle_ui_event(&mut self, event: UiEvent) {
        let handler = ScriptEventHandler {
            runtime: &mut self.runtime,
        };
        let mut driver = EventDriver::new(&mut self.inner, handler);
        driver.handle_ui_event(event);
        self.runtime.ctx.flush_wrapper_switches();

        // JS may have mutated the DOM or scheduled timers
        self.request_redraw();
        self.arm_timer_thread();
    }

    fn poll(&mut self, task_context: Option<TaskContext>) -> bool {
        // Store the waker so the timer thread can wake the event loop
        if let Some(cx) = &task_context {
            let mut waker = self.waker.lock().unwrap();
            let stale = waker
                .as_ref()
                .map(|old| !old.will_wake(cx.waker()))
                .unwrap_or(true);
            if stale {
                *waker = Some(cx.waker().clone());
            }
        }

        // Execute scripts on first poll if they haven't been run explicitly
        let mut ran = false;
        if !self.scripts_executed {
            self.execute_scripts();
            ran = true;
        }

        ran |= self.runtime.run_due_timers();
        self.runtime.ctx.flush_wrapper_switches();
        self.arm_timer_thread();
        ran
    }
}
