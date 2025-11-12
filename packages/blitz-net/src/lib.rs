//! Networking (HTTP, filesystem, Data URIs) for Blitz
//!
//! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

use blitz_traits::net::{
    Body, BoxedHandler, Bytes, NetCallback, NetProvider, Request, SharedCallback,
};
use data_url::DataUrl;
use std::sync::Arc;
use tokio::{
    runtime::Handle,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

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
    let cwd = std::env::current_dir().unwrap();
    let cache_dir = cwd.join(".blitz-cache");

    if !std::fs::exists(&cache_dir).unwrap() {
        std::fs::create_dir(&cache_dir).unwrap();
    }

    cache_dir
}

pub struct Provider<D> {
    rt: Handle,
    client: Client,
    resource_callback: SharedCallback<D>,
}
impl<D: 'static> Provider<D> {
    pub fn new(resource_callback: SharedCallback<D>) -> Self {
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

        Self {
            rt: Handle::current(),
            client,
            resource_callback,
        }
    }
    pub fn shared(res_callback: SharedCallback<D>) -> Arc<dyn NetProvider<D>> {
        Arc::new(Self::new(res_callback))
    }
    pub fn is_empty(&self) -> bool {
        Arc::strong_count(&self.resource_callback) == 1
    }
    pub fn count(&self) -> usize {
        Arc::strong_count(&self.resource_callback) - 1
    }
}
impl<D: 'static> Provider<D> {
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

    async fn fetch_with_handler(
        client: Client,
        request: Request,
        handler: BoxedHandler,
    ) -> Result<(), ProviderError> {
        let (response_url, bytes) = Self::fetch_inner(client, request).await?;
        handler.bytes(response_url, bytes);
        Ok(())
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

impl<D: 'static> NetProvider<D> for Provider<D> {
    fn fetch(&self, _doc_id: usize, request: Request, handler: BoxedHandler) {
        let client = self.client.clone();

        #[cfg(feature = "debug_log")]
        println!("Fetching {}", &request.url);

        self.rt.spawn(async move {
            #[cfg(feature = "debug_log")]
            let url = request.url.to_string();

            let _res = Self::fetch_with_handler(client, request, handler).await;

            #[cfg(feature = "debug_log")]
            if let Err(e) = _res {
                eprintln!("Error fetching {url}: {e:?}");
            } else {
                println!("Success {url}");
            }
        });
    }
}

#[derive(Debug)]
pub enum ProviderError {
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

pub struct MpscCallback<T>(UnboundedSender<(usize, T)>);
impl<T> MpscCallback<T> {
    pub fn new() -> (UnboundedReceiver<(usize, T)>, Self) {
        let (send, recv) = unbounded_channel();
        (recv, Self(send))
    }
}
impl<T: Send + Sync + 'static> NetCallback<T> for MpscCallback<T> {
    fn call(&self, doc_id: usize, result: Result<T, Option<String>>) {
        // TODO: handle error case
        if let Ok(data) = result {
            let _ = self.0.send((doc_id, data));
        }
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
