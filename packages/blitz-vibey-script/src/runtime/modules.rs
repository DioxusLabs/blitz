//! Synchronous ES module loading, caching, and evaluation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use boa_engine::builtins::promise::PromiseState;
use boa_engine::module::{Module, ModuleLoader, ModuleRequest, Referrer};
use boa_engine::{Context, JsError, JsNativeError, JsResult, Source};
use url::Url;

use super::{ScriptRuntime, events::report_js_error};
use crate::fetch::{FetchError, ScriptFetcher, document_may_load};

/// A [`ModuleLoader`] which fetches ES module imports synchronously via the
/// document's [`ScriptFetcher`], resolving specifiers as URLs (relative to the
/// importing module, or to the document base URL).
pub(super) struct BlitzModuleLoader {
    fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    base_url: Option<Url>,
    /// Modules already fetched and parsed, keyed by resolved URL. Serving
    /// repeat imports from here breaks import cycles (e.g. `a.js` importing
    /// `b.js` which imports `a.js`), which would otherwise load forever.
    modules: RefCell<HashMap<Url, Module>>,
}

impl BlitzModuleLoader {
    pub(super) fn new(fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>, base_url: Option<Url>) -> Self {
        Self {
            fetcher,
            base_url,
            modules: RefCell::new(HashMap::new()),
        }
    }

    fn resolve_specifier(&self, referrer: &Referrer, specifier: &str) -> Option<Url> {
        if let Ok(url) = Url::parse(specifier) {
            return Some(url);
        }
        let referrer_url = referrer
            .path()
            .and_then(|path| path.to_str())
            .and_then(|path| Url::parse(path).ok());
        let base = referrer_url.or_else(|| self.base_url.clone())?;
        base.join(specifier).ok()
    }

    fn get(&self, url: &Url) -> Option<Module> {
        self.modules.borrow().get(url).cloned()
    }

    fn insert(&self, url: Url, module: Module) {
        self.modules.borrow_mut().insert(url, module);
    }
}

impl ModuleLoader for BlitzModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        request: ModuleRequest,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let specifier = request.specifier().to_std_string_escaped();
        let url = self
            .resolve_specifier(&referrer, &specifier)
            .ok_or_else(|| {
                JsError::from(
                    JsNativeError::typ()
                        .with_message(format!("could not resolve module specifier {specifier:?}")),
                )
            })?;
        if let Some(module) = self.get(&url) {
            return Ok(module);
        }
        let result = if document_may_load(self.base_url.as_ref(), &url) {
            self.fetcher.borrow().fetch(&url)
        } else {
            Err(FetchError::Forbidden(url.clone()))
        };
        let code = result.map_err(|error| {
            JsError::from(
                JsNativeError::typ().with_message(format!("failed to fetch module {url}: {error}")),
            )
        })?;
        let source = Source::from_reader(code.as_bytes(), Some(Path::new(url.as_str())));
        let module = Module::parse(source, None, &mut context.borrow_mut())?;
        self.insert(url, module.clone());
        Ok(module)
    }
}

impl ScriptRuntime {
    /// Evaluate an ES module script: parse it, load its imports (via the module
    /// loader), then link and evaluate it. Uncaught errors are logged (but not
    /// propagated), matching [`eval`](Self::eval).
    pub fn eval_module(&mut self, code: &str, url: Option<&Url>) {
        let description = url
            .map(Url::as_str)
            .unwrap_or("<inline module>")
            .to_string();
        let path = url.map(|url| Path::new(url.as_str()));
        let source = Source::from_reader(code.as_bytes(), path);

        let module = match Module::parse(source, None, &mut self.context) {
            Ok(module) => module,
            Err(error) => {
                report_js_error(&self.ctx, &mut self.context, &description, &error);
                return;
            }
        };
        // Register the module so that imports resolving to this URL (including
        // circular ones) reuse it rather than re-fetching
        if let Some(url) = url
            && self.module_loader.get(url).is_none()
        {
            self.module_loader.insert(url.clone(), module.clone());
        }

        let promise = module.load_link_evaluate(&mut self.context);
        self.run_jobs(&description);
        if let PromiseState::Rejected(reason) = promise.state() {
            report_js_error(
                &self.ctx,
                &mut self.context,
                &description,
                &JsError::from_opaque(reason),
            );
        }
    }
}
