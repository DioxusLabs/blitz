use crate::{NetProvider, RequestHandler, USER_AGENT};
use std::cell::RefCell;
use std::io::Read;
use thiserror::Error;
use url::Url;

const FILE_SIZE_LIMIT: u64 = 1_000_000_000; // 1GB

pub struct SyncProvider<T>(pub RefCell<Vec<T>>);
impl<T> SyncProvider<T> {
    pub fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
    fn fetch_inner<H: RequestHandler<T>>(
        &self,
        url: Url,
        handler: &H,
    ) -> Result<Vec<u8>, SyncProviderError> {
        Ok(match url.scheme() {
            "data" => {
                let data_url = data_url::DataUrl::process(url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                decoded.0
            }
            "file" => {
                let file_content = std::fs::read(url.path())?;
                file_content
            }
            _ => {
                let response = ureq::request(handler.method().as_str(), url.as_str())
                    .set("User-Agent", USER_AGENT)
                    .call()?;

                let len: usize = response
                    .header("Content-Length")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
                let mut bytes: Vec<u8> = Vec::with_capacity(len);
                response
                    .into_reader()
                    .take(FILE_SIZE_LIMIT)
                    .read_to_end(&mut bytes)?;
                bytes
            }
        })
    }
}
impl<T> NetProvider<T> for SyncProvider<T> {
    fn fetch<H>(&self, url: Url, handler: H)
    where
        H: RequestHandler<T>,
    {
        let res = match self.fetch_inner(url, &handler) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{e}");
                return;
            }
        };
        self.0.borrow_mut().push(handler.bytes(&res));
    }
}

#[derive(Error, Debug)]
enum SyncProviderError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    DataUrl(#[from] data_url::DataUrlError),
    #[error("{0}")]
    DataUrlBas64(#[from] data_url::forgiving_base64::InvalidBase64),
    #[error("{0}")]
    Ureq(#[from] ureq::Error),
}
