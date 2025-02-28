use std::sync::Arc;

use crate::{ProviderError, USER_AGENT};

use super::{BackendError, RequestBackend, Response};
use blitz_traits::net::{BoxedHandler, NetProvider, Request, SharedCallback};
use data_url::DataUrl;
use http::HeaderValue;
use tokio::{
    runtime::Handle,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
};

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

impl RequestBackend for Backend {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            client: reqwest::Client::new(),
        }
    }

    async fn request(&mut self, request: Request) -> Result<Response, BackendError> {
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
