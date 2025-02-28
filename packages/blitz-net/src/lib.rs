use backend::{Backend, RequestBackend};
use blitz_traits::net::{BoxedHandler, Bytes, Request, SharedCallback};
use data_url::DataUrl;
use http::HeaderValue;
use thiserror::Error;

use url::Url;

#[cfg(all(feature = "reqwest", feature = "ureq"))]
compile_error!("multiple request backends cannot be enabled at the same time");

mod backend;
pub mod callback;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub async fn get_text(url: &str) -> String {
    let mut backend = Backend::new();
    let request = Request::get(Url::parse(url).unwrap());
    let response = backend.request(request).await.unwrap();
    String::from_utf8_lossy(&response.body).to_string()
}

pub use backend::Provider;

impl<D: 'static> Provider<D> {
    async fn fetch_inner(
        mut client: Backend,
        doc_id: usize,
        request: Request,
        handler: BoxedHandler<D>,
        res_callback: SharedCallback<D>,
    ) -> Result<(), ProviderError> {
        match request.url.scheme() {
            "data" => {
                let data_url = DataUrl::process(request.url.as_str())?;
                let decoded = data_url.decode_to_vec()?;
                handler.bytes(doc_id, Bytes::from(decoded.0), res_callback);
            }
            "file" => {
                let file_content = std::fs::read(request.url.path())?;
                handler.bytes(doc_id, Bytes::from(file_content), res_callback);
            }
            _ => {
                let mut request = Request::get(request.url);
                request
                    .headers
                    .insert("User-Agent", HeaderValue::from_static(USER_AGENT));
                let response = client.request(request).await?;

                handler.bytes(doc_id, response.body, res_callback);
            }
        }
        Ok(())
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
    BackendError(#[from] backend::BackendError),
}
