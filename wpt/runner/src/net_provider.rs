use blitz_traits::net::{BoxedHandler, Bytes, NetCallback, NetProvider, Request};
use data_url::DataUrl;
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

static REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

pub struct WptNetProvider<D: Send + Sync + 'static> {
    base_path: PathBuf,
    queue: Arc<InternalQueue<D>>,
}
impl<D: Send + Sync + 'static> WptNetProvider<D> {
    pub fn new(base_path: &Path) -> Self {
        let base_path = base_path.to_path_buf();
        Self {
            base_path,
            queue: Arc::new(InternalQueue::new()),
        }
    }

    pub fn pending_item_count(&self) -> usize {
        self.queue.pending_item_count()
    }

    pub fn log_pending_items(&self) {
        self.queue.log_pending_items();
    }

    pub fn for_each(&self, cb: impl FnMut(D)) {
        self.queue.for_each(cb);
    }

    /// Clear request state (any in-flight requests will be ignored)
    pub fn reset(&self) {
        self.queue.reset();
    }

    fn fetch_inner(
        &self,
        doc_id: usize,
        request_id: usize,
        request: Request,
        handler: BoxedHandler<D>,
    ) -> Result<(), WptNetProviderError> {
        let callback = Arc::new(Callback {
            queue: self.queue.clone(),
            request_id,
        });

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
                catch_unwind(AssertUnwindSafe(|| {
                    handler.bytes(doc_id, Bytes::from(file_content), callback)
                }))
                .map_err(|err| {
                    let str_msg = err.downcast_ref::<&str>().map(|s| s.to_string());
                    let string_msg = err.downcast_ref::<String>().map(|s| s.to_string());
                    let panic_msg = str_msg.or(string_msg);
                    WptNetProviderError::HandlerPanic(panic_msg)
                })?;
            }
        }
        Ok(())
    }
}
impl<D: Send + Sync + 'static> NetProvider<D> for WptNetProvider<D> {
    fn fetch(&self, doc_id: usize, request: Request, handler: BoxedHandler<D>) {
        let url = request.url.to_string();

        // println!("Loading {url}");

        let request_id = self.queue.create_request(request.url.to_string());
        let res = self.fetch_inner(doc_id, request_id, request, handler);
        if let Err(e) = res {
            self.queue.record_failure(request_id);
            // if !matches!(e, WptNetProviderError::Io(_)) {
            eprintln!("Error loading {url}: {e:?}");
            // }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum WptNetProviderError {
    Io(std::io::Error),
    DataUrl(data_url::DataUrlError),
    DataUrlBase64(data_url::forgiving_base64::InvalidBase64),
    HandlerPanic(Option<String>),
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

#[derive(Debug)]
enum RequestStatus {
    InProgress,
    Success,
    Error,
}

struct RequestState<D> {
    url: String,
    status: RequestStatus,
    data: Option<D>,
}

impl<D> RequestState<D> {
    fn new(_id: usize, url: String) -> Self {
        Self {
            url,
            status: RequestStatus::InProgress,
            data: None,
        }
    }
}

struct InternalQueue<T> {
    requests: Mutex<HashMap<usize, RequestState<T>>>,
}
impl<T> InternalQueue<T> {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(HashMap::new()),
        }
    }

    pub fn reset(&self) {
        self.requests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }

    pub fn create_request(&self, url: String) -> usize {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let state = RequestState::new(request_id, url);
        self.requests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(request_id, state);
        request_id
    }

    pub fn record_success(&self, data: T, request_id: usize) {
        let mut requests = self.requests.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(req) = requests.get_mut(&request_id) {
            // println!("Loaded {}", req.url);
            req.status = RequestStatus::Success;
            req.data = Some(data);
        }
    }

    pub fn record_failure(&self, request_id: usize) {
        let mut requests = self.requests.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(req) = requests.get_mut(&request_id) {
            println!("Error loading {}", req.url);
            req.status = RequestStatus::Error;
        }
    }

    pub fn pending_item_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    pub fn log_pending_items(&self) {
        let requests = self.requests.lock().unwrap_or_else(|err| err.into_inner());
        for (id, req) in requests.iter() {
            println!("Req {}: {} ({:?})", id, req.url, req.status);
        }
    }

    pub fn for_each(&self, mut cb: impl FnMut(T)) {
        // Note: we use a temporary Vec here so that the mutex is unlocked prior to any of the callbacks being called.
        // This prevents the mutex from being poisoned if any of the callbacks panic, allowing it to be reused for further tests.
        //
        // TODO: replace .retain with .extract_if once Rust 1.87 is stable
        let mut requests = self.requests.lock().unwrap_or_else(|err| err.into_inner());
        let mut completed: Vec<Result<T, ()>> = Vec::new();
        requests.retain(|_id, req| match req.status {
            RequestStatus::InProgress => true,
            RequestStatus::Success => {
                let data = req.data.take().unwrap();
                completed.push(Ok(data));
                false
            }
            RequestStatus::Error => {
                completed.push(Err(()));
                false
            }
        });
        drop(requests);

        for data in completed.into_iter().flatten() {
            cb(data);
        }
    }
}

struct Callback<T> {
    queue: Arc<InternalQueue<T>>,
    request_id: usize,
}

impl<T: Send + Sync + 'static> NetCallback<T> for Callback<T> {
    fn call(&self, _doc_id: usize, result: Result<T, Option<String>>) {
        match result {
            Ok(data) => self.queue.record_success(data, self.request_id),
            Err(err) => {
                if let Some(msg) = err {
                    eprintln!("{msg}");
                }
                self.queue.record_failure(self.request_id);
            }
        }
    }
}
