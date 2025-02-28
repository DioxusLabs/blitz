use std::sync::Arc;

use crate::{ProviderError, USER_AGENT};

use super::BackendError;
use blitz_traits::net::{BoxedHandler, Bytes, NetProvider, Request, Response, SharedCallback};
use data_url::DataUrl;
use http::HeaderValue;
use tokio::runtime::Handle;
use url::Url;

// Compat with reqwest
impl From<reqwest::Error> for BackendError {
    fn from(e: reqwest::Error) -> Self {
        BackendError {
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    client: reqwest::Client,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn request(&mut self, request: Request) -> Result<Response, BackendError> {
        let request = self
            .client
            .request(request.method, request.url.clone())
            .headers(request.headers);

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await?;

        Ok(Response {
            status: status.as_u16(),
            headers,
            body,
        })
    }
}

pub async fn get_text(url: &str) -> String {
    let mut backend = Backend::new();
    let request = Request::get(Url::parse(url).unwrap());
    let response = backend.request(request).await.unwrap();
    String::from_utf8_lossy(&response.body).to_string()
}

pub struct Provider<D> {
    rt: Handle,
    client: Backend,
    resource_callback: SharedCallback<D>,
}

impl<D: 'static> Provider<D> {
    pub fn new(res_callback: SharedCallback<D>) -> Self {
        Self {
            rt: Handle::current(),
            client: Backend::new(),
            resource_callback: res_callback,
        }
    }

    pub fn shared(res_callback: SharedCallback<D>) -> Arc<dyn NetProvider<Data = D>> {
        Arc::new(Self::new(res_callback))
    }

    pub fn is_empty(&self) -> bool {
        Arc::strong_count(&self.resource_callback) == 1
    }

    async fn fetch_inner(
        mut client: Backend,
        doc_id: usize,
        request: Request,
        handler: BoxedHandler<D>,
        res_callback: SharedCallback<D>,
    ) -> Result<(), ProviderError> {
        match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(doc_id, Bytes::from(decoded.0), res_callback);
            }
            "file" => {
                let file_content = std::fs::read(request.url.path())?;
                handler.bytes(doc_id, Bytes::from(file_content), res_callback);
            }
            _ => {
                let mut request = Request::get(request.url);
                request
                    .headers
                    .insert("User-Agent", HeaderValue::from_static(USER_AGENT));
                let response = client.request(request).await?;

                handler.bytes(doc_id, response.body, res_callback);
            }
        }
        Ok(())
    }
}

impl<D: 'static> NetProvider for Provider<D> {
    type Data = D;

    fn fetch(&self, doc_id: usize, request: Request, handler: BoxedHandler<D>) {
        let client = self.client.clone();
        let callback = Arc::clone(&self.resource_callback);
        println!("Fetching {}", &request.url);
        drop(self.rt.spawn(async move {
            let url = request.url.to_string();
            let res = Self::fetch_inner(client, doc_id, request, handler, callback).await;
            if let Err(e) = res {
                eprintln!("Error fetching {}: {e}", url);
            } else {
                println!("Success {}", url);
            }
        }));
    }
}
