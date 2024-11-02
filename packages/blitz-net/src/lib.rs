use blitz_traits::net::{BoxedHandler, Bytes, Callback, NetProvider, SharedCallback, Url};
use data_url::DataUrl;
use reqwest::Client;
use std::sync::Arc;
use thiserror::Error;
use tokio::{
    runtime::Handle,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub struct Provider<D> {
    rt: Handle,
    client: Client,
    resource_callback: SharedCallback<D>,
}
impl<D> Provider<D> {
    pub fn new(rt_handle: Handle, res_callback: SharedCallback<D>) -> Self {
        Self {
            rt: rt_handle,
            client: Client::new(),
            resource_callback: res_callback,
        }
    }
    pub fn is_empty(&self) -> bool {
        Arc::strong_count(&self.resource_callback) == 1
    }
}
impl<D: 'static> Provider<D> {
    async fn fetch_inner(
        client: Client,
        url: Url,
        handler: BoxedHandler<D>,
        res_callback: SharedCallback<D>,
    ) -> Result<(), ProviderError> {
        match url.scheme() {
            "data" => {
                let data_url = DataUrl::process(url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(Bytes::from(decoded.0), res_callback);
            }
            "file" => {
                let file_content = std::fs::read(url.path())?;
                handler.bytes(Bytes::from(file_content), res_callback);
            }
            _ => {
                let response = client
                    .request(handler.method(), url)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await?;
                handler.bytes(response.bytes().await?, res_callback);
            }
        }
        Ok(())
    }
}

impl<D: 'static> NetProvider for Provider<D> {
    type Data = D;
    fn fetch(&self, url: Url, handler: BoxedHandler<D>) {
        let client = self.client.clone();
        let callback = Arc::clone(&self.resource_callback);
        drop(self.rt.spawn(async {
            let res = Self::fetch_inner(client, url, handler, callback).await;
            if let Err(e) = res {
                eprintln!("{e}");
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
    ReqwestError(#[from] reqwest::Error),
}

pub struct MpscCallback<T>(UnboundedSender<T>);
impl<T> MpscCallback<T> {
    pub fn new() -> (UnboundedReceiver<T>, Self) {
        let (send, recv) = unbounded_channel();
        (recv, Self(send))
    }
}
impl<T: Send + Sync + 'static> Callback for MpscCallback<T> {
    type Data = T;
    fn call(&self, data: Self::Data) {
        let _ = self.0.send(data);
    }
}
