use crate::{NetProvider, RequestHandler, USER_AGENT};
use data_url::DataUrl;
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::Client;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    runtime::{Handle, Runtime},
    sync::Mutex,
    task::JoinHandle,
};
use url::Url;
use winit::{event_loop::EventLoopProxy, window::WindowId};

type TaskHandle<T> = JoinHandle<Result<T, AsyncProviderError>>;

pub struct AsyncProvider<T> {
    rt: Handle,
    client: Client,
    futures: Mutex<FuturesUnordered<TaskHandle<T>>>,
}
impl<T: Send + Sync + 'static> AsyncProvider<T> {
    pub fn new(rt: &Runtime) -> Self {
        Self {
            rt: rt.handle().clone(),
            client: Client::new(),
            futures: Mutex::new(FuturesUnordered::new()),
        }
    }
    pub fn resolve<P: From<(WindowId, T)> + Send>(
        self: Arc<Self>,
        event_loop_proxy: EventLoopProxy<P>,
        window_id: WindowId,
    ) {
        self.rt.clone().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));

            'thread: loop {
                interval.tick().await;
                while let Some(ir) = self.futures.lock().await.next().await {
                    match ir {
                        Ok(Ok(t)) => {
                            let e = event_loop_proxy.send_event((window_id, t).into());
                            if e.is_err() {
                                break 'thread;
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Fetch failed with {e:?}")
                        }
                        Err(e) => {
                            tracing::error!("Fetch thread failed with {e}")
                        }
                    }
                }
            }
        });
    }
}
impl<T: Send + 'static> AsyncProvider<T> {
    async fn fetch_inner<H: RequestHandler<T>>(
        client: Client,
        url: Url,
        handler: H,
    ) -> Result<T, AsyncProviderError> {
        match url.scheme() {
            "data" => {
                let data_url = DataUrl::process(url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                Ok(handler.bytes(&decoded.0))
            }
            "file" => {
                let file_content = std::fs::read(url.path())?;
                Ok(handler.bytes(&file_content))
            }
            _ => {
                let start = tokio::time::Instant::now();

                let response = client
                    .request(handler.method(), url.clone())
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await?;
                let res = handler.bytes(&response.bytes().await?);
                tracing::info!(
                    "Loaded {} in: {}ms",
                    url.as_str(),
                    start.elapsed().as_millis()
                );
                Ok(res)
            }
        }
    }
}

impl<T: Send + 'static> NetProvider<T> for AsyncProvider<T> {
    fn fetch<H>(&self, url: Url, handler: H)
    where
        H: RequestHandler<T>,
    {
        let client = self.client.clone();
        let join = self.rt.spawn(Self::fetch_inner(client, url, handler));
        self.futures.blocking_lock().push(join);
    }
}

#[derive(Error, Debug)]
enum AsyncProviderError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    DataUrl(#[from] data_url::DataUrlError),
    #[error("{0}")]
    DataUrlBas64(#[from] data_url::forgiving_base64::InvalidBase64),
    #[error("{0}")]
    ReqwestError(#[from] reqwest::Error),
}
