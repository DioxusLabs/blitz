use blitz_traits::net::{BoxedHandler, Bytes, NetProvider, Request, Response, SharedCallback};
use data_url::DataUrl;
use http::HeaderValue;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::Arc;
use url::Url;

use crate::{ProviderError, USER_AGENT};

use super::BackendError;

impl From<ureq::Error> for BackendError {
    fn from(e: ureq::Error) -> Self {
        BackendError {
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    client: ureq::Agent,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            client: ureq::agent(),
        }
    }

    pub fn request(&mut self, mut request: Request) -> Result<Response, BackendError> {
        request
            .headers
            .insert("User-Agent", HeaderValue::from_static(&USER_AGENT));

        let mut response = if request.body.is_empty() {
            self.client
                .run(<blitz_traits::net::Request as Into<http::Request<()>>>::into(request))?
        } else {
            self.client.run(<blitz_traits::net::Request as Into<
                http::Request<Vec<u8>>,
            >>::into(request))?
        };
        let status = response.status().as_u16();

        Ok(Response {
            status,
            headers: response.headers().clone(),
            body: response.body_mut().read_to_vec()?.into(),
        })
    }
}

pub fn get_text(url: &str) -> String {
    let mut backend = Backend::new();
    let request = Request::get(Url::parse(url).unwrap());
    let response = backend.request(request).unwrap();
    String::from_utf8_lossy(&response.body).to_string()
}

pub struct Provider<D> {
    thread_pool: ThreadPool,
    client: Backend,
    resource_callback: SharedCallback<D>,
}

impl<D: 'static> Provider<D> {
    pub fn new(res_callback: SharedCallback<D>) -> Self {
        let thread_pool = ThreadPoolBuilder::new().num_threads(0).build().unwrap();

        Self {
            thread_pool,
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

    fn fetch_inner(
        mut client: Backend,
        doc_id: usize,
        request: Request,
        handler: BoxedHandler<D>,
        callback: SharedCallback<D>,
    ) -> Result<(), ProviderError> {
        match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(doc_id, Bytes::from(decoded.0), callback);
            }
            "file" => {
                let file_content = std::fs::read(request.url.path())?;
                handler.bytes(doc_id, Bytes::from(file_content), callback);
            }
            _ => {
                let mut request = Request::get(request.url);
                request
                    .headers
                    .insert("User-Agent", HeaderValue::from_static(USER_AGENT));
                let response = client.request(request)?;

                handler.bytes(doc_id, response.body, callback);
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
        self.thread_pool.spawn(move || {
            let url = request.url.to_string();
            let res = Self::fetch_inner(client, doc_id, request, handler, callback);
            if let Err(e) = res {
                eprintln!("Error fetching {}: {e}", url);
            } else {
                println!("Success {}", url);
            }
        });
    }
}
