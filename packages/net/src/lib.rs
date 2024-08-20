use std::cell::{RefCell, UnsafeCell};
use std::future::Future;
use std::ops::Deref;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use futures_util::future::Map;
use futures_util::{FutureExt, StreamExt};
use reqwest::{Client, Request};
use tokio::runtime::{Handle, Runtime};
pub use url::Url;
use futures_util::stream::FuturesUnordered;
use tokio::task::{JoinError, JoinHandle};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub struct Net<I, T> {
    rt: Handle,
    client: Client,
    pub futures: UnsafeCell<FuturesUnordered<JoinMap<I, T>>>,
}
impl<I: Send + Sync, T: Send + Sync> Net<I, T> {
    pub fn new(rt: &Runtime) -> Self {
        Self {
            rt: rt.handle().clone(),
            client: Client::new(),
            futures: UnsafeCell::new(FuturesUnordered::new()),
        }
    }
    pub async fn resolve<P: From<(WindowId, (I, T))>>(this: Arc<Self>, event_loop_proxy: EventLoopProxy<P>, window_id: WindowId) {
        loop {
            while let Some(ir) = unsafe { &mut *this.futures.get()}.next().await {
                if let Some(ir) = ir {
                    let _ = event_loop_proxy.send_event((window_id, ir).into());
                } 
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

type JoinMap<I, T> = Map<JoinHandle<(I, T)>, fn(Result<(I, T), JoinError>)->Option<(I, T)>>;

impl<I: Send + 'static, T: Send + 'static> dom::util::NetProvider<I, T> for Net<I, T> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static
    {
        let client = self.client.clone();
        unsafe { &*self.futures.get() }.push(self.rt.spawn(async move {
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
                    let response = client.get(url)
                        .header("User-Agent", USER_AGENT)
                        .send()
                        .await
                        .unwrap();
                    (i, handler(response.bytes().await.unwrap().deref()))
                }
            }
        }).map(Result::ok));
    }
}

unsafe impl<I: Send, T: Send> Send for Net<I, T> {}
unsafe impl<I: Sync, T: Sync> Sync for Net<I, T> {}

