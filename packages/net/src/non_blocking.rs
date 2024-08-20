use super::{NetProvider, USER_AGENT};
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use reqwest::Client;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use url::Url;
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

pub struct AsyncProvider<I, T> {
    rt: Handle,
    client: Client,
    futures: Mutex<FuturesUnordered<JoinHandle<(I, T)>>>,
}
impl<I: Send + Sync, T: Send + Sync> AsyncProvider<I, T> {
    pub fn new(rt: &Runtime) -> Self {
        Self {
            rt: rt.handle().clone(),
            client: Client::new(),
            futures: Mutex::new(FuturesUnordered::new()),
        }
    }
    pub async fn resolve<P: From<(WindowId, (I, T))>>(
        self: Arc<Self>,
        event_loop_proxy: EventLoopProxy<P>,
        window_id: WindowId,
    ) {
        loop {
            while let Some(ir) = self.futures.lock().await.next().await {
                if let Ok(ir) = ir {
                    let _ = event_loop_proxy.send_event((window_id, ir).into());
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

impl<I: Send + 'static, T: Send + 'static> NetProvider<I, T> for AsyncProvider<I, T> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        let client = self.client.clone();
        self.futures.blocking_lock().push(self.rt.spawn(async move {
            match url.scheme() {
                "data" => {
                    let data_url = data_url::DataUrl::process(url.as_str()).unwrap();
                    let decoded = data_url.decode_to_vec().expect("Invalid data url");
                    (i, handler(decoded.0.deref()))
                }
                "file" => {
                    let file_content = std::fs::read(url.path()).unwrap();
                    (i, handler(file_content.deref()))
                }
                _ => {
                    let url_str = url.as_str().to_string();
                    let start = tokio::time::Instant::now();
                    let response = client
                        .get(url)
                        .header("User-Agent", USER_AGENT)
                        .send()
                        .await
                        .unwrap();
                    let res = handler(response.bytes().await.unwrap().deref());
                    println!("Loaded {} in: {}ms", url_str, start.elapsed().as_millis());
                    (i, res)
                }
            }
        }));
    }
}

unsafe impl<I: Send, T: Send> Send for AsyncProvider<I, T> {}
unsafe impl<I: Sync, T: Sync> Sync for AsyncProvider<I, T> {}
