use blitz_traits::net::{BoxedHandler, Bytes, NetCallback, NetProvider, Request, SharedCallback};
use data_url::DataUrl;
use reqwest::Client;
use std::sync::Arc;
use tokio::{
    runtime::Handle,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub async fn get_text(url: &str) -> String {
    Client::new()
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

pub struct Provider<D> {
    rt: Handle,
    client: Client,
    resource_callback: SharedCallback<D>,
}
impl<D: 'static> Provider<D> {
    pub fn new(res_callback: SharedCallback<D>) -> Self {
        Self {
            rt: Handle::current(),
            client: Client::new(),
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
        client: Client,
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
                let response = client
                    .request(request.method, request.url)
                    .headers(request.headers)
                    .header("User-Agent", USER_AGENT)
                    .body(request.body)
                    .send()
                    .await?;

                handler.bytes(doc_id, response.bytes().await?, res_callback);
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
        self.rt.spawn(async move {
            let url = request.url.to_string();
            let res = Self::fetch_inner(client, doc_id, request, handler, callback).await;
            if let Err(e) = res {
                eprintln!("Error fetching {}: {:?}", url, e);
            } else {
                println!("Success {}", url);
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
impl<T: Send + Sync + 'static> NetCallback for MpscCallback<T> {
    type Data = T;
    fn call(&self, doc_id: usize, data: Self::Data) {
        let _ = self.0.send((doc_id, data));
    }
}
