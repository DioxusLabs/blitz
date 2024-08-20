use super::{NetProvider, USER_AGENT};
use std::cell::RefCell;
use std::io::Read;
use url::Url;

const FILE_SIZE_LIMIT: u64 = 1_000_000_000; // 1GB

pub struct SyncProvider<I, T>(pub RefCell<Vec<(I, T)>>);
impl<I, T> SyncProvider<I, T> {
    pub fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
}
impl<I, T> NetProvider<I, T> for SyncProvider<I, T> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T,
    {
        let res = match url.scheme() {
            "data" => {
                let data_url = data_url::DataUrl::process(url.as_str()).unwrap();
                let decoded = data_url.decode_to_vec().expect("Invalid data url");
                decoded.0
            }
            "file" => {
                let file_content = std::fs::read(url.path()).unwrap();
                file_content
            }
            _ => {
                let response = ureq::get(url.as_str())
                    .set("User-Agent", USER_AGENT)
                    .call()
                    .map_err(Box::new);

                let Ok(response) = response else {
                    tracing::error!("{}", response.unwrap_err());
                    return;
                };
                let len: usize = response
                    .header("Content-Length")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
                let mut bytes: Vec<u8> = Vec::with_capacity(len);

                response
                    .into_reader()
                    .take(FILE_SIZE_LIMIT)
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            }
        };
        self.0.borrow_mut().push((i, handler(&res)));
    }
}
