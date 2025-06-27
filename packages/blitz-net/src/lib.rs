//! Networking (HTTP, filesystem, Data URIs) for Blitz
//!
//! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

use blitz_traits::net::{
    AbortSignal, BoxedHandler, Bytes, NetCallback, NetProvider, Request, SharedCallback,
};
use data_url::DataUrl;
use reqwest::Client;
use std::{marker::PhantomData, pin::Pin, sync::Arc, task::Poll};
use tokio::{
    runtime::Handle,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub struct Provider<D> {
    rt: Handle,
    client: Client,
    resource_callback: SharedCallback<D>,
}
impl<D: 'static> Provider<D> {
    pub fn new(resource_callback: SharedCallback<D>) -> Self {
        #[cfg(feature = "cookies")]
        let client = Client::builder().cookie_store(true).build().unwrap();
        #[cfg(not(feature = "cookies"))]
        let client = Client::new();

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
                    .header("User-Agent", USER_AGENT)
                    .body(request.body)
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
        let client = self.client.clone();
        self.rt.spawn(async move {
            let url = request.url.to_string();
            let result = Self::fetch_inner(client, request).await;
            if let Err(e) = &result {
                eprintln!("Error fetching {url}: {e:?}");
            } else {
                println!("Success {url}");
            }
            callback(result);
        });
    }

    pub async fn fetch_async(&self, request: Request) -> Result<(String, Bytes), ProviderError> {
        let client = self.client.clone();
        let url = request.url.to_string();
        let result = Self::fetch_inner(client, request).await;
        if let Err(e) = &result {
            eprintln!("Error fetching {url}: {e:?}");
        } else {
            println!("Success {url}");
        }
        result
    }
}

impl<D: 'static> NetProvider<D> for Provider<D> {
    fn fetch(&self, doc_id: usize, mut request: Request, handler: BoxedHandler<D>) {
        let client = self.client.clone();
        let callback = Arc::clone(&self.resource_callback);
        println!("Fetching {}", &request.url);
        self.rt.spawn(async move {
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

            match result {
                Ok((_response_url, bytes)) => {
                    handler.bytes(doc_id, bytes, callback);
                    println!("Success {url}");
                }
                Err(e) => {
                    eprintln!("Error fetching {url}: {e:?}");
                }
            }
        });
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
