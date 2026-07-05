//! Synchronous fetching of external script sources (`<script src="...">`)

use std::fmt;

use url::Url;

#[derive(Debug)]
pub enum FetchError {
    UnsupportedScheme(String),
    Io(std::io::Error),
    InvalidData(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => {
                write!(f, "unsupported URL scheme for script: {scheme}")
            }
            Self::Io(error) => write!(f, "IO error fetching script: {error}"),
            Self::InvalidData(msg) => write!(f, "invalid script data: {msg}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Trait for synchronously fetching external script sources.
///
/// Scripts are fetched synchronously because the HTML spec requires classic
/// scripts to execute in document order, blocking parsing.
pub trait ScriptFetcher: 'static {
    fn fetch(&self, url: &Url) -> Result<String, FetchError>;
}

/// The default [`ScriptFetcher`]: supports `file:` and `data:` URLs.
pub struct DefaultScriptFetcher;

impl ScriptFetcher for DefaultScriptFetcher {
    fn fetch(&self, url: &Url) -> Result<String, FetchError> {
        match url.scheme() {
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| FetchError::InvalidData(format!("invalid file URL: {url}")))?;
                std::fs::read_to_string(path).map_err(FetchError::Io)
            }
            "data" => {
                let data_url = data_url::DataUrl::process(url.as_str())
                    .map_err(|err| FetchError::InvalidData(format!("{err:?}")))?;
                let (bytes, _) = data_url
                    .decode_to_vec()
                    .map_err(|err| FetchError::InvalidData(format!("{err:?}")))?;
                String::from_utf8(bytes).map_err(|err| FetchError::InvalidData(format!("{err:?}")))
            }
            scheme => Err(FetchError::UnsupportedScheme(scheme.to_string())),
        }
    }
}
