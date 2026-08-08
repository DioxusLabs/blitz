//! Networking (HTTP, filesystem, Data URIs) for Blitz
//!
//! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

use blitz_traits::net::{
    AbortSignal, Body, ByteFetcher, Bytes, FetchError, FetcherProvider, NetHandler, NetProvider,
    NetWaker, Request, RequestObserver,
};
use data_url::DataUrl;
use std::{
    collections::HashMap,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};
use tokio::sync::Semaphore;

#[cfg(feature = "cache")]
use http_cache_reqwest::{
    CACacheManager, Cache, CacheMode, CacheOptions, HttpCache, HttpCacheOptions,
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

/// Matches real browsers' per-origin cap of 6.
const PER_HOST_MAX_CONCURRENT: usize = 6;

type HostLimits = Arc<Mutex<HashMap<String, Arc<Semaphore>>>>;

#[cfg(feature = "cache")]
type Client = reqwest_middleware::ClientWithMiddleware;
#[cfg(not(feature = "cache"))]
type Client = reqwest::Client;

#[cfg(feature = "cache")]
type RequestBuilder = reqwest_middleware::RequestBuilder;
#[cfg(not(feature = "cache"))]
type RequestBuilder = reqwest::RequestBuilder;

#[cfg(feature = "cache")]
fn get_cache_path() -> std::path::PathBuf {
    use directories::ProjectDirs;
    let path = ProjectDirs::from("com", "DioxusLabs", "Blitz")
        .expect("Failed to find cache directory")
        .cache_dir()
        .to_owned();
    #[cfg(feature = "tracing")]
    tracing::info!(path = ?path.display(), "Using cache dir");
    path
}

#[cfg(target_arch = "wasm32")]
fn spawn(fut: impl Future + 'static) {
    wasm_bindgen_futures::spawn_local(async move {
        fut.await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn<F>(fut: F)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(fut);
}

pub struct HttpFetcher {
    client: Client,
    per_host_limits: HostLimits,
    #[cfg(feature = "cache")]
    cache_manager: CACacheManager,
}
impl HttpFetcher {
    pub fn new() -> Self {
        let builder = reqwest::Client::builder();
        #[cfg(feature = "cookies")]
        let builder = builder.cookie_store(true);
        let client = builder.build().unwrap();

        #[cfg(feature = "cache")]
        let cache_manager = CACacheManager::new(get_cache_path(), true);

        #[cfg(feature = "cache")]
        let client = reqwest_middleware::ClientBuilder::new(client)
            .with(Cache(HttpCache {
                mode: CacheMode::Default,
                manager: cache_manager.clone(),
                options: HttpCacheOptions {
                    // Evaluate cache policy as a single-user (private) cache, like a
                    // real browser, rather than a shared/proxy cache. The default
                    // (`shared: true`) treats any response carrying `Set-Cookie` without
                    // an explicit `Cache-Control: public`/`immutable` as immediately
                    // stale, forcing a revalidation request to the server on every load.
                    // Many CDNs (e.g. Wikimedia image hosts) serve images this way, so
                    // the shared-cache default defeats disk caching and gets us rate
                    // limited. A private cache honours heuristic freshness instead.
                    cache_options: Some(CacheOptions {
                        shared: false,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }))
            .build();

        Self {
            client,
            per_host_limits: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(feature = "cache")]
            cache_manager,
        }
    }
    #[cfg(feature = "cache")]
    pub async fn clear_cache(&self) {
        if let Err(e) = self.cache_manager.clear().await {
            #[cfg(feature = "tracing")]
            tracing::error!("Failed to clear HTTP cache: {:?}", e);
            #[cfg(not(feature = "tracing"))]
            let _ = e;
        }
    }
}

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Provider {
    fetcher: Arc<HttpFetcher>,
    inner: FetcherProvider<Arc<HttpFetcher>>,
}
impl Provider {
    pub fn new(waker: Option<Arc<dyn NetWaker>>) -> Self {
        let fetcher = Arc::new(HttpFetcher::new());
        let inner = FetcherProvider::new(fetcher.clone(), waker);
        Self { fetcher, inner }
    }
    pub fn shared(waker: Option<Arc<dyn NetWaker>>) -> Arc<dyn NetProvider> {
        Arc::new(Self::new(waker))
    }
    pub fn is_empty(&self) -> bool {
        Arc::strong_count(self.inner.waker()) == 1
    }
    pub fn count(&self) -> usize {
        Arc::strong_count(self.inner.waker()) - 1
    }

    #[cfg(feature = "cache")]
    pub async fn clear_cache(&self) {
        self.fetcher.clear_cache().await;
    }

    pub fn set_observer(&self, observer: Arc<dyn RequestObserver>) {
        self.inner.set_observer(observer);
    }
}
impl HttpFetcher {
    async fn fetch_inner(
        client: Client,
        request: Request,
        per_host_limits: HostLimits,
    ) -> Result<(String, Bytes), ProviderError> {
        match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                Ok((request.url.to_string(), Bytes::from(decoded.0)))
            }
            "file" => {
                let file_content = std::fs::read(request.url.path())?;
                Ok((request.url.to_string(), Bytes::from(file_content)))
            }
            _ => Self::fetch_http(client, request, per_host_limits).await,
        }
    }

    async fn fetch_http(
        client: Client,
        request: Request,
        per_host_limits: HostLimits,
    ) -> Result<(String, Bytes), ProviderError> {
        // Acquire a per-host permit, held for the duration of the request, to
        // keep total in-flight requests per origin bounded.
        let host_key = request
            .url
            .host_str()
            .map(str::to_owned)
            .unwrap_or_default();
        let semaphore = {
            let mut map = per_host_limits.lock().unwrap();
            map.entry(host_key)
                .or_insert_with(|| Arc::new(Semaphore::new(PER_HOST_MAX_CONCURRENT)))
                .clone()
        };
        let _permit = semaphore
            .acquire()
            .await
            .expect("per-host semaphore was closed");

        let mut req = client
            .request(request.method, request.url)
            .headers(request.headers)
            .header("User-Agent", USER_AGENT);

        if let Some(content_type) = request.content_type.as_ref() {
            req = req.header("Content-Type", content_type);
        }

        let req = req
            .apply_body(request.body, request.content_type.as_deref())
            .await;
        let response = req.send().await?;
        let status = response.status();
        let final_url = response.url().to_string();

        if status.is_success() {
            return Ok((final_url, response.bytes().await?));
        }

        #[cfg(feature = "tracing")]
        tracing::warn!(
            url = final_url.as_str(),
            status = status.as_u16(),
            "HTTP error status"
        );
        Err(ProviderError::HttpStatus {
            status,
            url: final_url,
        })
    }

    #[allow(clippy::type_complexity)]
    pub fn fetch_with_callback(
        &self,
        request: Request,
        callback: Box<dyn FnOnce(Result<(String, Bytes), ProviderError>) + Send + Sync + 'static>,
    ) {
        #[cfg(feature = "tracing")]
        let url = request.url.to_string();

        let client = self.client.clone();
        let per_host_limits = self.per_host_limits.clone();
        spawn(async move {
            let result = Self::fetch_inner(client, request, per_host_limits).await;

            #[cfg(feature = "tracing")]
            if let Err(e) = &result {
                #[cfg(feature = "tracing")]
                tracing::error!(url = url.as_str(), error = ?e, "Fetching");
            } else {
                #[cfg(feature = "tracing")]
                tracing::info!(url = url.as_str(), "Success fetching");
            }

            callback(result);
        });
    }

    pub async fn fetch_async(&self, request: Request) -> Result<(String, Bytes), ProviderError> {
        #[cfg(feature = "tracing")]
        let url = request.url.to_string();

        let client = self.client.clone();
        let per_host_limits = self.per_host_limits.clone();
        let result = Self::fetch_inner(client, request, per_host_limits).await;

        #[cfg(feature = "tracing")]
        if let Err(e) = &result {
            #[cfg(feature = "tracing")]
            tracing::error!(url = url.as_str(), error = ?e, "Fetching");
        } else {
            #[cfg(feature = "tracing")]
            tracing::info!(url = url.as_str(), "Success fetching");
        }

        result
    }
}

impl ByteFetcher for HttpFetcher {
    fn fetch_bytes(
        &self,
        request: Request,
        on_done: Box<dyn FnOnce(Result<(String, Bytes), FetchError>) + Send>,
    ) {
        #[cfg(feature = "tracing")]
        let url = request.url.to_string();

        let client = self.client.clone();
        let per_host_limits = self.per_host_limits.clone();
        let signal = request.signal.clone();
        spawn(async move {
            let result = if let Some(signal) = signal {
                AbortFetch::new(
                    signal,
                    Box::pin(async move {
                        HttpFetcher::fetch_inner(client, request, per_host_limits).await
                    }),
                )
                .await
            } else {
                HttpFetcher::fetch_inner(client, request, per_host_limits).await
            };

            #[cfg(feature = "tracing")]
            if let Err(e) = &result {
                tracing::error!(url = url.as_str(), error = ?e, "Fetching");
            } else {
                tracing::info!(url = url.as_str(), "Success fetching");
            }

            on_done(result.map_err(|error| match error {
                ProviderError::Abort => FetchError::Aborted,
                error => FetchError::new(error),
            }));
        });
    }
}

impl Provider {
    #[allow(clippy::type_complexity)]
    pub fn fetch_with_callback(
        &self,
        request: Request,
        callback: Box<dyn FnOnce(Result<(String, Bytes), ProviderError>) + Send + Sync + 'static>,
    ) {
        self.fetcher.fetch_with_callback(request, callback);
    }

    pub async fn fetch_async(&self, request: Request) -> Result<(String, Bytes), ProviderError> {
        self.fetcher.fetch_async(request).await
    }
}

impl NetProvider for Provider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        #[cfg(feature = "tracing")]
        tracing::info!(url = request.url.as_str(), "Fetching");
        self.inner.fetch(doc_id, request, handler);
    }
}

struct AbortFetch<F, T> {
    signal: AbortSignal,
    future: F,
    _rt: PhantomData<T>,
}

impl<F, T> AbortFetch<F, T> {
    fn new(signal: AbortSignal, future: F) -> Self {
        Self {
            signal,
            future,
            _rt: PhantomData,
        }
    }
}

impl<F, T> Future for AbortFetch<F, T>
where
    F: Future + Unpin + 'static,
    F::Output: Into<Result<T, ProviderError>> + 'static,
    T: Unpin,
{
    type Output = Result<T, ProviderError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.signal.aborted() {
            return Poll::Ready(Err(ProviderError::Abort));
        }

        match Pin::new(&mut self.future).poll(cx) {
            Poll::Ready(output) => Poll::Ready(output.into()),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub enum ProviderError {
    Abort,
    Io(std::io::Error),
    DataUrl(data_url::DataUrlError),
    DataUrlBase64(data_url::forgiving_base64::InvalidBase64),
    ReqwestError(reqwest::Error),
    #[cfg(feature = "cache")]
    ReqwestMiddlewareError(reqwest_middleware::Error),
    HttpStatus {
        status: reqwest::StatusCode,
        url: String,
    },
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Abort => write!(f, "request aborted"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::DataUrl(e) => write!(f, "data url error: {e:?}"),
            Self::DataUrlBase64(e) => write!(f, "data url base64 error: {e:?}"),
            Self::ReqwestError(e) => write!(f, "reqwest error: {e}"),
            #[cfg(feature = "cache")]
            Self::ReqwestMiddlewareError(e) => write!(f, "reqwest middleware error: {e}"),
            Self::HttpStatus { status, url } => write!(f, "HTTP {status} for {url}"),
        }
    }
}

impl From<std::io::Error> for ProviderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<data_url::DataUrlError> for ProviderError {
    fn from(value: data_url::DataUrlError) -> Self {
        Self::DataUrl(value)
    }
}

impl From<data_url::forgiving_base64::InvalidBase64> for ProviderError {
    fn from(value: data_url::forgiving_base64::InvalidBase64) -> Self {
        Self::DataUrlBase64(value)
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(value)
    }
}

#[cfg(feature = "cache")]
impl From<reqwest_middleware::Error> for ProviderError {
    fn from(value: reqwest_middleware::Error) -> Self {
        Self::ReqwestMiddlewareError(value)
    }
}

trait ReqwestExt {
    async fn apply_body(self, body: Body, content_type: Option<&str>) -> Self;
}
impl ReqwestExt for RequestBuilder {
    async fn apply_body(self, body: Body, content_type: Option<&str>) -> Self {
        match body {
            Body::Bytes(bytes) => self.body(bytes),
            Body::Form(form_data) => match content_type {
                Some("application/x-www-form-urlencoded") => self.form(&form_data),
                #[cfg(feature = "multipart")]
                Some("multipart/form-data") => {
                    use blitz_traits::net::Entry;
                    use blitz_traits::net::EntryValue;
                    let mut form_data = form_data;
                    let mut form = reqwest::multipart::Form::new();
                    for Entry { name, value } in form_data.0.drain(..) {
                        form = match value {
                            EntryValue::String(value) => form.text(name, value),
                            EntryValue::File(path_buf) => form
                                .file(name, path_buf)
                                .await
                                .expect("Couldn't read form file from disk"),
                            EntryValue::EmptyFile => form.part(
                                name,
                                reqwest::multipart::Part::bytes(&[])
                                    .mime_str("application/octet-stream")
                                    .unwrap(),
                            ),
                        };
                    }
                    self.multipart(form)
                }
                _ => self,
            },
            Body::Empty => self,
        }
    }
}
