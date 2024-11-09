use blitz_traits::net::{BoxedHandler, Bytes, Callback, NetProvider, Url};
use data_url::DataUrl;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub struct WptNetProvider<D: Send + Sync + 'static> {
    base_path: PathBuf,
    callback: Arc<VecCallback<D>>,
}
impl<D: Send + Sync + 'static> WptNetProvider<D> {
    pub fn new(base_path: &Path) -> Self {
        let base_path = base_path.to_path_buf();
        Self {
            base_path,
            callback: Arc::new(VecCallback::new()),
        }
    }

    pub fn for_each(&self, cb: impl FnMut(D)) {
        self.callback.for_each(cb);
    }

    fn fetch_inner(&self, url: Url, handler: BoxedHandler<D>) -> Result<(), WptNetProviderError> {
        let callback = self.callback.clone();
        match url.scheme() {
            "data" => {
                let data_url = DataUrl::process(url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(Bytes::from(decoded.0), callback);
            }
            _ => {
                let relative_path = url.path().strip_prefix('/').unwrap();
                let path = self.base_path.join(relative_path);
                let file_content = std::fs::read(&path).inspect_err(|err| {
                    eprintln!("Error loading {}: {}", path.display(), &err);
                })?;
                handler.bytes(Bytes::from(file_content), callback);
            }
        }
        Ok(())
    }
}
impl<D: Send + Sync + 'static> NetProvider for WptNetProvider<D> {
    type Data = D;
    fn fetch(&self, url: Url, handler: BoxedHandler<D>) {
        let res = self.fetch_inner(url.clone(), handler);
        if let Err(e) = res {
            if !matches!(e, WptNetProviderError::Io(_)) {
                eprintln!("Error loading {}: {e}", url);
            }
        }
    }
}

#[derive(Error, Debug)]
enum WptNetProviderError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    DataUrl(#[from] data_url::DataUrlError),
    #[error("{0}")]
    DataUrlBase64(#[from] data_url::forgiving_base64::InvalidBase64),
}

struct VecCallback<T>(Mutex<Vec<T>>);
impl<T> VecCallback<T> {
    pub fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    pub fn for_each(&self, mut cb: impl FnMut(T)) {
        let mut data = self.0.lock().unwrap();
        for item in data.drain(0..) {
            cb(item)
        }
    }
}
impl<T: Send + Sync + 'static> Callback for VecCallback<T> {
    type Data = T;
    fn call(&self, data: Self::Data) {
        self.0.lock().unwrap().push(data)
    }
}
