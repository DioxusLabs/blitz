//! Synchronous fetching of external script sources (`<script src="...">`)

use std::fmt;

use url::Url;

#[derive(Debug)]
pub enum FetchError {
    UnsupportedScheme(String),
    /// The requesting document is not allowed to load this URL
    /// (see [`document_may_load`])
    Forbidden(Url),
    Io(std::io::Error),
    InvalidData(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => {
                write!(f, "unsupported URL scheme for script: {scheme}")
            }
            Self::Forbidden(url) => write!(f, "not allowed to load {url} from this document"),
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

/// Whether a document at `document_url` may load `url` (via `<script src>`,
/// `import` or `fetch()`). `file:` URLs may only be loaded by `file:` documents,
/// so that remote pages cannot read local files.
pub fn document_may_load(document_url: Option<&Url>, url: &Url) -> bool {
    url.scheme() != "file"
        || document_url.is_some_and(|document_url| document_url.scheme() == "file")
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
