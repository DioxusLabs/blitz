use std::fmt::{Display, Formatter};

use thiserror::Error;

#[cfg(feature = "reqwest")]
mod reqwest;
#[cfg(feature = "ureq")]
mod ureq;

#[cfg(feature = "reqwest")]
pub use reqwest::{get_text, Provider};
#[cfg(feature = "ureq")]
pub use ureq::{get_text, Provider};

#[derive(Debug, Error)]
pub struct BackendError {
    pub message: String,
}

impl Display for BackendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
