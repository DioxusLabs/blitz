use blitz_traits::net::{BoxedHandler, Bytes, NetCallback, NetProvider, Request};
use data_url::DataUrl;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

pub struct WptNetProvider<D: Send + Sync + 'static> {
    base_path: PathBuf,
    request_count: AtomicUsize,
    callback: Arc<VecCallback<D>>,
}
impl<D: Send + Sync + 'static> WptNetProvider<D> {
    pub fn new(base_path: &Path) -> Self {
        let base_path = base_path.to_path_buf();
        Self {
            base_path,
            request_count: AtomicUsize::new(0),
            callback: Arc::new(VecCallback::new()),
        }
    }

    pub fn pending_item_count(&self) -> usize {
        self.request_count.load(Ordering::SeqCst) - self.callback.processed_count()
    }

    pub fn for_each(&self, cb: impl FnMut(D)) {
        self.callback.for_each(cb);
    }

    fn fetch_inner(
        &self,
        doc_id: usize,
        request: Request,
        handler: BoxedHandler<D>,
    ) -> Result<(), WptNetProviderError> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let callback = self.callback.clone();
        match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(doc_id, Bytes::from(decoded.0), callback);
            }
            _ => {
                let relative_path = request.url.path().strip_prefix('/').unwrap();
                let path = self.base_path.join(relative_path);
                let file_content = std::fs::read(&path).inspect_err(|err| {
                    eprintln!("Error loading {}: {}", path.display(), &err);
                })?;
                handler.bytes(doc_id, Bytes::from(file_content), callback);
            }
        }
        Ok(())
    }
}
impl<D: Send + Sync + 'static> NetProvider for WptNetProvider<D> {
    type Data = D;
    fn fetch(&self, doc_id: usize, request: Request, handler: BoxedHandler<D>) {
        let url = request.url.to_string();

        let res = self.fetch_inner(doc_id, request, handler);
        if let Err(e) = res {
            if !matches!(e, WptNetProviderError::Io(_)) {
                eprintln!("Error loading {}: {:?}", url, e);
            }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum WptNetProviderError {
    Io(std::io::Error),
    DataUrl(data_url::DataUrlError),
    DataUrlBase64(data_url::forgiving_base64::InvalidBase64),
}

impl From<std::io::Error> for WptNetProviderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<data_url::DataUrlError> for WptNetProviderError {
    fn from(value: data_url::DataUrlError) -> Self {
        Self::DataUrl(value)
    }
}

impl From<data_url::forgiving_base64::InvalidBase64> for WptNetProviderError {
    fn from(value: data_url::forgiving_base64::InvalidBase64) -> Self {
        Self::DataUrlBase64(value)
    }
}

struct VecCallback<T> {
    processed_count: AtomicUsize,
    queue: Mutex<Vec<T>>,
}
impl<T> VecCallback<T> {
    pub fn new() -> Self {
        Self {
            processed_count: AtomicUsize::new(0),
            queue: Mutex::new(Vec::new()),
        }
    }

    pub fn processed_count(&self) -> usize {
        self.processed_count.load(Ordering::SeqCst)
    }

    pub fn for_each(&self, mut cb: impl FnMut(T)) {
        // Note: we use std::mem::take here so that the mutex is unlocked prior to any of the callbacks being called.
        // This prevents the mutex from being poisoned if any of the callbacks panic, allowing it to be reused for further tests.
        //
        // TODO: Cleanup still-in-flight requests in case of panic.
        let mut data = std::mem::take(&mut *self.queue.lock().unwrap());
        self.processed_count.fetch_add(data.len(), Ordering::SeqCst);
        for item in data.drain(0..) {
            cb(item)
        }
    }
}
impl<T: Send + Sync + 'static> NetCallback for VecCallback<T> {
    type Data = T;
    fn call(&self, _doc_id: usize, data: Self::Data) {
        self.queue
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(data)
    }
}
