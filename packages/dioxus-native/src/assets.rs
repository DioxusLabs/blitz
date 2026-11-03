use blitz_shell::BlitzShellProxy;
use std::sync::Arc;

use blitz_traits::net::{NetHandler, NetProvider, Request};

pub struct DioxusNativeNetProvider {
    inner_net_provider: Option<Arc<dyn NetProvider + 'static>>,
}

#[allow(unused)]
impl DioxusNativeNetProvider {
    pub fn shared(proxy: BlitzShellProxy) -> Arc<dyn NetProvider> {
        Arc::new(Self::new(proxy)) as Arc<dyn NetProvider>
    }

    pub fn new(proxy: BlitzShellProxy) -> Self {
        #[cfg(any(feature = "data-uri", feature = "net"))]
        let net_waker = Some(Arc::new(proxy) as _);

        #[cfg(feature = "net")]
        let inner_net_provider = Some(blitz_net::Provider::shared(net_waker.clone()));
        #[cfg(all(feature = "data-uri", not(feature = "net")))]
        let inner_net_provider = Some(blitz_shell::DataUriNetProvider::shared(net_waker.clone()));
        #[cfg(all(not(feature = "data-uri"), not(feature = "net")))]
        let inner_net_provider = None;

        Self { inner_net_provider }
    }

    pub fn with_inner(proxy: BlitzShellProxy, inner: Arc<dyn NetProvider>) -> Self {
        Self {
            inner_net_provider: Some(inner),
        }
    }

    pub fn inner(&self) -> Option<&Arc<dyn NetProvider>> {
        self.inner_net_provider.as_ref()
    }
}

impl NetProvider for DioxusNativeNetProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if request.url.scheme() == "dioxus" {
            let raw_path = request.url.path();
            let decoded_path = percent_encoding::percent_decode_str(raw_path).decode_utf8_lossy();
            let decoded_path_str: &str = &decoded_path;

            if let Ok(res) = dioxus_asset_resolver::native::serve_asset(raw_path)
                && res.status().is_success()
            {
                #[cfg(feature = "tracing")]
                tracing::trace!("fetching asset from file system success {request:#?}");
                handler.bytes(request.url.to_string(), res.into_body().into());
                return;
            }

            if decoded_path_str != raw_path
                && let Ok(res) = dioxus_asset_resolver::native::serve_asset(decoded_path_str)
                && res.status().is_success()
            {
                #[cfg(feature = "tracing")]
                tracing::trace!("fetching asset from file system success {request:#?}");
                handler.bytes(request.url.to_string(), res.into_body().into());
                return;
            }

            let normalized_path = decoded_path_str.trim_start_matches('/');
            let alt_path = normalized_path.strip_prefix("assets/");

            let mut candidates = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut add_candidate = |path: std::path::PathBuf| {
                if seen.insert(path.clone()) {
                    candidates.push(path);
                }
            };

            // 1. Direct absolute path candidates (Dev-mode macro expansion & file URLs)
            let direct_path = std::path::Path::new(decoded_path_str);
            if direct_path.is_absolute() {
                add_candidate(direct_path.to_path_buf());
            }
            let raw_direct = std::path::Path::new(raw_path);
            if raw_direct.is_absolute() {
                add_candidate(raw_direct.to_path_buf());
            }
            #[cfg(windows)]
            {
                let trimmed_win = decoded_path_str.trim_start_matches('/');
                let win_path = std::path::Path::new(trimmed_win);
                if win_path.is_absolute() {
                    add_candidate(win_path.to_path_buf());
                }
            }

            // 2. Relative candidate search
            let mut push_candidates_for_base = |base: &std::path::Path| {
                add_candidate(base.join(normalized_path));
                if let Some(stripped) = alt_path {
                    add_candidate(base.join(stripped));
                } else {
                    add_candidate(base.join("assets").join(normalized_path));
                }

                if raw_path != decoded_path_str {
                    let raw_normalized = raw_path.trim_start_matches('/');
                    add_candidate(base.join(raw_normalized));
                    if let Some(stripped) = raw_normalized.strip_prefix("assets/") {
                        add_candidate(base.join(stripped));
                    } else {
                        add_candidate(base.join("assets").join(raw_normalized));
                    }
                }
            };

            if let Ok(current_dir) = std::env::current_dir() {
                push_candidates_for_base(&current_dir);
            }

            if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                push_candidates_for_base(std::path::Path::new(&manifest_dir));
            }

            if let Ok(exe_path) = std::env::current_exe() {
                let mut current = exe_path.as_path();
                for _ in 0..4 {
                    if let Some(parent) = current.parent() {
                        push_candidates_for_base(parent);
                        current = parent;
                    } else {
                        break;
                    }
                }
            }

            for candidate_path in candidates {
                if candidate_path.is_file()
                    && let Ok(data) = std::fs::read(&candidate_path)
                {
                    #[cfg(feature = "tracing")]
                    tracing::info!(
                        "Resolved unbundled asset {:?} from filesystem: {:?}",
                        request.url.path(),
                        candidate_path
                    );
                    handler.bytes(request.url.to_string(), data.into());
                    return;
                }
            }

            #[cfg(feature = "tracing")]
            tracing::warn!(
                "Failed to resolve asset {:?}: not found in bundle or dev paths",
                request.url
            );
        } else if let Some(inner) = &self.inner_net_provider {
            inner.fetch(doc_id, request, handler);
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!("net feature not enabled, cannot fetch {request:#?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_traits::net::{Bytes, NetHandler, Request, Url};
    use std::sync::{Arc, Mutex};

    struct CapturingNetHandler {
        dispatched: Arc<Mutex<Option<(String, Bytes)>>>,
    }

    impl NetHandler for CapturingNetHandler {
        fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
            *self.dispatched.lock().unwrap() = Some((resolved_url, bytes));
        }
    }

    struct TempFileGuard(std::path::PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_unbundled_asset_resolution_success() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("test_style_{}.css", std::process::id()));
        std::fs::write(&test_file, b"body { margin: 0; }").unwrap();
        let _guard = TempFileGuard(test_file.clone());

        let provider = DioxusNativeNetProvider {
            inner_net_provider: None,
        };
        let dispatched = Arc::new(Mutex::new(None));
        let handler = Box::new(CapturingNetHandler {
            dispatched: Arc::clone(&dispatched),
        });

        let url = Url::parse(&format!("dioxus://{}", test_file.to_str().unwrap())).unwrap();
        provider.fetch(0, Request::get(url), handler);

        let result = dispatched.lock().unwrap().take();

        assert!(
            result.is_some(),
            "Asset should be resolved via unbundled filesystem fallback"
        );
        assert_eq!(result.unwrap().1.as_ref(), b"body { margin: 0; }");
    }

    #[test]
    fn test_unbundled_asset_relative_search_success() {
        let file_name = format!("test_relative_asset_{}.css", std::process::id());
        let current_dir = std::env::current_dir().unwrap();
        let test_file = current_dir.join(&file_name);
        std::fs::write(&test_file, b"div { color: red; }").unwrap();
        let _guard = TempFileGuard(test_file);

        let provider = DioxusNativeNetProvider {
            inner_net_provider: None,
        };
        let dispatched = Arc::new(Mutex::new(None));
        let handler = Box::new(CapturingNetHandler {
            dispatched: Arc::clone(&dispatched),
        });

        let url = Url::parse(&format!("dioxus:///{file_name}")).unwrap();
        provider.fetch(0, Request::get(url), handler);

        let result = dispatched.lock().unwrap().take();

        assert!(
            result.is_some(),
            "Asset should be resolved via candidate search relative to current_dir"
        );
        assert_eq!(result.unwrap().1.as_ref(), b"div { color: red; }");
    }

    #[test]
    fn test_404_asset_no_byte_leakage() {
        let provider = DioxusNativeNetProvider {
            inner_net_provider: None,
        };
        let dispatched = Arc::new(Mutex::new(None));
        let handler = Box::new(CapturingNetHandler {
            dispatched: Arc::clone(&dispatched),
        });

        let url = Url::parse("dioxus:///non_existent_asset_xyz.css").unwrap();
        provider.fetch(0, Request::get(url), handler);

        let result = dispatched.lock().unwrap().take();
        assert!(
            result.is_none(),
            "Non-existent asset must NOT dispatch 'Not Found' bytes to NetHandler"
        );
    }

    #[test]
    fn test_unbundled_asset_direct_absolute_path_success() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("test_direct_abs_{}.css", std::process::id()));
        std::fs::write(&test_file, b"h1 { font-size: 20px; }").unwrap();
        let _guard = TempFileGuard(test_file.clone());

        let provider = DioxusNativeNetProvider {
            inner_net_provider: None,
        };
        let dispatched = Arc::new(Mutex::new(None));
        let handler = Box::new(CapturingNetHandler {
            dispatched: Arc::clone(&dispatched),
        });

        let file_path_str = test_file.to_str().unwrap().trim_start_matches('/');
        let url = Url::parse(&format!("dioxus:///{file_path_str}")).unwrap();
        provider.fetch(0, Request::get(url), handler);

        let result = dispatched.lock().unwrap().take();
        assert!(
            result.is_some(),
            "Asset should be resolved via direct absolute path probe"
        );
        assert_eq!(result.unwrap().1.as_ref(), b"h1 { font-size: 20px; }");
    }

    #[test]
    fn test_unbundled_asset_percent_encoded_spaces_success() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("test space asset {}.css", std::process::id()));
        std::fs::write(&test_file, b"p { color: blue; }").unwrap();
        let _guard = TempFileGuard(test_file.clone());

        let provider = DioxusNativeNetProvider {
            inner_net_provider: None,
        };
        let dispatched = Arc::new(Mutex::new(None));
        let handler = Box::new(CapturingNetHandler {
            dispatched: Arc::clone(&dispatched),
        });

        let file_path_str = test_file.to_str().unwrap().trim_start_matches('/');
        let encoded_path = file_path_str.replace(' ', "%20");
        let url = Url::parse(&format!("dioxus:///{encoded_path}")).unwrap();
        provider.fetch(0, Request::get(url), handler);

        let result = dispatched.lock().unwrap().take();
        assert!(
            result.is_some(),
            "Asset with percent-encoded spaces should be resolved"
        );
        assert_eq!(result.unwrap().1.as_ref(), b"p { color: blue; }");
    }
}
