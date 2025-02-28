use std::fmt::{Display, Formatter};

use blitz_traits::net::{Request, Response};
use thiserror::Error;

#[cfg(feature = "reqwest")]
mod reqwest;
#[cfg(feature = "ureq")]
mod ureq;

#[cfg(feature = "reqwest")]
pub use reqwest::Backend;
#[cfg(feature = "reqwest")]
pub use reqwest::Provider;

#[cfg(feature = "ureq")]
pub use ureq::Backend;

pub trait RequestBackend {
    fn new() -> Self
    where
        Self: Sized;
    async fn request(&mut self, request: Request) -> Result<Response, BackendError>;
}

#[derive(Debug, Error)]
pub struct BackendError {
    pub message: String,
}

impl Display for BackendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
