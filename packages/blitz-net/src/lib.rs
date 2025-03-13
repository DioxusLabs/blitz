use thiserror::Error;

#[cfg(all(feature = "reqwest", feature = "ureq"))]
compile_error!("multiple request backends cannot be enabled at the same time. either use reqwest or ureq, but not both");

mod backend;
pub mod callback;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub use backend::{get_text, Provider};

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
