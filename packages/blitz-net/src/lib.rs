use backend::{Backend, RequestBackend};
use blitz_traits::net::{BoxedHandler, Bytes, NetCallback, NetProvider, Request, SharedCallback};
use data_url::DataUrl;
use http::HeaderValue;
use std::sync::Arc;
use thiserror::Error;
use tokio::{
    runtime::Handle,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
};
use url::Url;

#[cfg(all(feature = "reqwest", feature = "ureq"))]
compile_error!("multiple request backends cannot be enabled at the same time");

mod backend;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

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
}
impl<D: 'static> Provider<D> {
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

#[derive(Error, Debug)]
enum ProviderError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    DataUrl(#[from] data_url::DataUrlError),
    #[error("{0}")]
    DataUrlBas64(#[from] data_url::forgiving_base64::InvalidBase64),
    #[error("{0}")]
    BackendError(#[from] backend::BackendError),
}

pub struct MpscCallback<T>(UnboundedSender<(usize, T)>);
impl<T> MpscCallback<T> {
    pub fn new() -> (UnboundedReceiver<(usize, T)>, Self) {
        let (send, recv) = unbounded_channel();
        (recv, Self(send))
    }
}
impl<T: Send + Sync + 'static> NetCallback for MpscCallback<T> {
    type Data = T;
    fn call(&self, doc_id: usize, data: Self::Data) {
        let _ = self.0.send((doc_id, data));
    }
}
