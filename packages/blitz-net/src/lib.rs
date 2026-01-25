//! Networking (HTTP, filesystem, Data URIs) for Blitz
//!
//! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

// use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, NetWaker, Request};
use blitz_traits::net::{AbortSignal, Body, Bytes, NetHandler, NetProvider, NetWaker, Request};
use data_url::DataUrl;
use std::{marker::PhantomData, pin::Pin, sync::Arc, task::Poll};
use tokio::runtime::Handle;

#[cfg(feature = "cache")]
use http_cache_reqwest::{CACacheManager, Cache, CacheMode, HttpCache, HttpCacheOptions};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

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
    println!("Using cache dir {}", path.display());
    path
}

pub struct Provider {
    rt: Handle,
    client: Client,
    waker: Arc<dyn NetWaker>,
}
impl Provider {
    pub fn new(waker: Option<Arc<dyn NetWaker>>) -> Self {
        let builder = reqwest::Client::builder();
        #[cfg(feature = "cookies")]
        let builder = builder.cookie_store(true);
        let client = builder.build().unwrap();

        #[cfg(feature = "cache")]
        let client = reqwest_middleware::ClientBuilder::new(client)
            .with(Cache(HttpCache {
                mode: CacheMode::Default,
                manager: CACacheManager::new(get_cache_path(), true),
                options: HttpCacheOptions::default(),
            }))
            .build();

        let waker = waker.unwrap_or(Arc::new(DummyNetWaker));
        Self {
            rt: Handle::current(),
            client,
            waker,
        }
    }
    pub fn shared(waker: Option<Arc<dyn NetWaker>>) -> Arc<dyn NetProvider> {
        Arc::new(Self::new(waker))
    }
    pub fn is_empty(&self) -> bool {
        Arc::strong_count(&self.waker) == 1
    }
    pub fn count(&self) -> usize {
        Arc::strong_count(&self.waker) - 1
    }
}
impl Provider {
    async fn fetch_inner(
        client: Client,
        request: Request,
    ) -> Result<(String, Bytes), ProviderError> {
        Ok(match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                (request.url.to_string(), Bytes::from(decoded.0))
            }
            "file" => {
                let file_content = std::fs::read(request.url.path())?;
                (request.url.to_string(), Bytes::from(file_content))
            }
            _ => {
                let response = client
                    .request(request.method, request.url)
                    .headers(request.headers)
                    .header("Content-Type", request.content_type.as_str())
                    .header("User-Agent", USER_AGENT)
                    .apply_body(request.body, request.content_type.as_str())
                    .await
                    .send()
                    .await?;

                (response.url().to_string(), response.bytes().await?)
            }
        })
    }

    #[allow(clippy::type_complexity)]
    pub fn fetch_with_callback(
        &self,
        request: Request,
        callback: Box<dyn FnOnce(Result<(String, Bytes), ProviderError>) + Send + Sync + 'static>,
    ) {
        #[cfg(feature = "debug_log")]
        let url = request.url.to_string();

        let client = self.client.clone();
        self.rt.spawn(async move {
            let result = Self::fetch_inner(client, request).await;

            #[cfg(feature = "debug_log")]
            if let Err(e) = &result {
                eprintln!("Error fetching {url}: {e:?}");
            } else {
                println!("Success {url}");
            }

            callback(result);
        });
    }

    pub async fn fetch_async(&self, request: Request) -> Result<(String, Bytes), ProviderError> {
        #[cfg(feature = "debug_log")]
        let url = request.url.to_string();

        let client = self.client.clone();
        let result = Self::fetch_inner(client, request).await;

        #[cfg(feature = "debug_log")]
        if let Err(e) = &result {
            eprintln!("Error fetching {url}: {e:?}");
        } else {
            println!("Success {url}");
        }

        result
    }
}

impl NetProvider for Provider {
    fn fetch(&self, doc_id: usize, mut request: Request, handler: Box<dyn NetHandler>) {
        let client = self.client.clone();

        #[cfg(feature = "debug_log")]
        println!("Fetching {}", &request.url);

        let waker = self.waker.clone();
        self.rt.spawn(async move {
            #[cfg(feature = "debug_log")]
            let url = request.url.to_string();

            let signal = request.signal.take();
            let result = if let Some(signal) = signal {
                AbortFetch::new(
                    signal,
                    Box::pin(async move { Self::fetch_inner(client, request).await }),
                )
                .await
            } else {
                Self::fetch_inner(client, request).await
            };

            // Call the waker to notify of completed network request
            waker.wake(doc_id);

            match result {
                Ok((response_url, bytes)) => {
                    handler.bytes(response_url, bytes);
                    #[cfg(feature = "debug_log")]
                    println!("Success {url}");
                }
                Err(e) => {
                    #[cfg(feature = "debug_log")]
                    eprintln!("Error fetching {url}: {e:?}");
                    #[cfg(not(feature = "debug_log"))]
                    let _ = e;
                }
            };
        });
    }
}

/// A future that is cancellable using an AbortSignal
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
    F: Future + Unpin + Send + 'static,
    F::Output: Send + Into<Result<T, ProviderError>> + 'static,
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
    async fn apply_body(self, body: Body, content_type: &str) -> Self;
}
impl ReqwestExt for RequestBuilder {
    async fn apply_body(self, body: Body, content_type: &str) -> Self {
        match body {
            Body::Bytes(bytes) => self.body(bytes),
            Body::Form(form_data) => match content_type {
                "application/x-www-form-urlencoded" => self.form(&form_data),
                #[cfg(feature = "multipart")]
                "multipart/form-data" => {
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

struct DummyNetWaker;
impl NetWaker for DummyNetWaker {
    fn wake(&self, _client_id: usize) {}
}
