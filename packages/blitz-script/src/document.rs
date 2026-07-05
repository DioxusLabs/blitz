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
    fetcher: Box<dyn ScriptFetcher>,
    scripts_executed: bool,

    // Timer wakeups: a background thread which wakes the event loop (via the
    // `Waker` passed to `poll`) when the next JS timer is due.
    waker: Arc<Mutex<Option<Waker>>>,
    timer_thread: Option<Sender<Instant>>,
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

        let base_url = config
            .base_url
            .as_deref()
            .and_then(|url| Url::parse(url).ok());

        let mut doc = BaseDocument::new(config);
        let mut mutr = doc.mutate();
        DocumentHtmlParser::parse_into_mutator(&mut mutr, html);
        drop(mutr);

        let inner = Rc::new(RefCell::new(doc));
        let runtime = ScriptRuntime::new(Rc::clone(&inner), base_url.as_ref());

        Self {
            inner,
            runtime,
            base_url,
            fetcher: Box::new(DefaultScriptFetcher),
            scripts_executed: false,
            waker: Arc::new(Mutex::new(None)),
            timer_thread: None,
        }
    }

    /// Override the [`ScriptFetcher`] used to load external (`src="..."`) scripts.
    /// The default fetcher supports `file:` and `data:` URLs.
    pub fn with_fetcher(mut self, fetcher: impl ScriptFetcher) -> Self {
        self.fetcher = Box::new(fetcher);
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
            match script.src {
                Some(src) => {
                    let resolved = match &self.base_url {
                        Some(base) => base.join(&src).ok(),
                        None => Url::parse(&src).ok(),
                    };
                    let Some(url) = resolved else {
                        eprintln!("blitz-script: could not resolve script URL {src:?}");
                        continue;
                    };
                    match self.fetcher.fetch(&url) {
                        Ok(code) => self.runtime.eval(&code, url.as_str()),
                        Err(error) => {
                            eprintln!("blitz-script: failed to fetch script {url}: {error}")
                        }
                    }
                }
                None => self.runtime.eval(&script.inline_text, "<inline script>"),
            }
        }

        self.runtime.dispatch_document_event("DOMContentLoaded");
        self.runtime.dispatch_window_event("load");

        self.request_redraw();
        self.arm_timer_thread();
    }

    /// Evaluate arbitrary JavaScript code in the document's script context
    pub fn eval(&mut self, code: &str) {
        self.runtime.eval(code, "<eval>");
        self.request_redraw();
        self.arm_timer_thread();
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
                    // Skip non-JavaScript script types (e.g. JSON data blocks).
                    // `module` scripts are treated as classic scripts for now.
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
        let Some(deadline) = self.runtime.next_timer_deadline() else {
            return;
        };

        let sender = self.timer_thread.get_or_insert_with(|| {
            let (tx, rx) = channel::<Instant>();
            let waker = Arc::clone(&self.waker);
            std::thread::Builder::new()
                .name("blitz-script-timers".to_string())
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
        self.arm_timer_thread();
        ran
    }
}
