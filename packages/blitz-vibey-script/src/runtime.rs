//! Runtime construction, script evaluation, and microtask execution.
//!
//! API installation lives in `bootstrap`; event dispatch, timers, module
//! loading, and window/CSS/fetch bindings have separate implementations.

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::{Context, Finalize, JsResult, Source, Trace};
use boa_runtime::Console;
use boa_runtime::console::{ConsoleState, Logger};
use url::Url;

use crate::fetch::ScriptFetcher;
use crate::state::{DomCtx, ReadyState};
use bootstrap::{BOOTSTRAP_JS, GEOMETRY_JS};
use events::report_js_error;
use modules::BlitzModuleLoader;

mod bootstrap;
mod css;
mod events;
mod fetch;
mod modules;
mod timers;
mod window;

/// Console logger which routes all console output to the `log` crate at
/// `debug` level (target `js_console`), keeping stdout/stderr clean.
#[derive(Debug, Trace, Finalize)]
struct LogCrateLogger;

impl Logger for LogCrateLogger {
    fn log(&self, msg: String, _state: &ConsoleState, _context: &mut Context) -> JsResult<()> {
        log::debug!(target: "js_console", "{msg}");
        Ok(())
    }

    fn info(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn warn(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn error(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }
}

pub(crate) struct ScriptRuntime {
    pub context: Context,
    pub ctx: DomCtx,
    module_loader: Rc<BlitzModuleLoader>,
}

impl ScriptRuntime {
    pub fn new(
        doc: Rc<RefCell<BaseDocument>>,
        base_url: Option<&Url>,
        fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    ) -> Self {
        let module_loader = Rc::new(BlitzModuleLoader::new(
            Rc::clone(&fetcher),
            base_url.cloned(),
        ));
        let ctx = DomCtx::new(doc);
        {
            let mut state = ctx.state.borrow_mut();
            state.base_url = base_url.cloned();
            state.fetcher = Some(fetcher);
        }
        // Share the runtime's clock with boa so that `Date` observes the same
        // (possibly virtual) time as timers
        let clock = ctx.state.borrow().clock.clone();
        let mut context = Context::builder()
            .module_loader(module_loader.clone())
            .clock(Rc::new(crate::clock::BoaClockAdapter::new(clock)))
            .build()
            .expect("failed to build JS context");
        context.insert_data(ctx.clone());

        Console::register_with_logger(LogCrateLogger, &mut context)
            .expect("failed to register console");

        // Register boa_runtime's web-API extensions: atob/btoa, TextEncoder/
        // TextDecoder, structuredClone, queueMicrotask and URL.
        //
        // Deliberately NOT registered:
        // - TimeoutExtension: blitz-vibey-script has its own setTimeout/setInterval/
        //   requestAnimationFrame implementation integrated with the document's
        //   event loop and timer thread
        // - FetchExtension/AbortControllerExtension: fetch should go through
        //   the embedder's net provider, not an internal HTTP client
        boa_runtime::register_extensions(
            (
                boa_runtime::extensions::Base64Extension,
                boa_runtime::extensions::EncodingExtension,
                boa_runtime::extensions::StructuredCloneExtension,
                boa_runtime::extensions::MicrotaskExtension,
                boa_runtime::extensions::UrlExtension,
            ),
            None,
            &mut context,
        )
        .expect("failed to register boa_runtime extensions");

        crate::dom::init_protos(&ctx, &mut context);

        bootstrap::register_globals(&ctx, base_url, &mut context);

        let mut runtime = Self {
            context,
            ctx,
            module_loader,
        };

        // Install JavaScript APIs after the native bindings.
        runtime.eval_internal(BOOTSTRAP_JS, "<blitz-bootstrap>");
        runtime.eval_internal(GEOMETRY_JS, "<blitz-geometry>");

        runtime
    }

    /// Evaluate a script, logging (but not propagating) any uncaught errors,
    /// then drain the microtask queue.
    pub fn eval(&mut self, code: &str, description: &str) {
        self.eval_internal(code, description);
        self.run_jobs(description);
    }

    fn eval_internal(&mut self, code: &str, description: &str) {
        if let Err(error) = self.context.eval(Source::from_bytes(code)) {
            report_js_error(&self.ctx, &mut self.context, description, &error);
        }
    }

    /// Run pending promise jobs (microtasks)
    pub fn run_jobs(&mut self, description: &str) {
        if let Err(error) = self.context.run_jobs() {
            report_js_error(&self.ctx, &mut self.context, description, &error);
        }
    }

    /// Set the value exposed as `document.readyState`
    pub fn set_ready_state(&mut self, ready_state: ReadyState) {
        self.ctx.state.borrow_mut().ready_state = ready_state;
    }
}
