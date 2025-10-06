use std::ops::Deref;
use std::str::FromStr;

use style::servo_arc::Arc as ServoArc;
use style::stylesheets::UrlExtraData;
use url::Url;

#[derive(Clone)]
pub(crate) struct DocumentUrl {
    base_url: ServoArc<Url>,
}

impl DocumentUrl {
    /// Create a stylo `UrlExtraData` from the URL
    pub(crate) fn url_extra_data(&self) -> UrlExtraData {
        UrlExtraData(ServoArc::clone(&self.base_url))
    }

    pub(crate) fn resolve_relative(&self, raw: &str) -> Option<url::Url> {
        self.base_url.join(raw).ok()
    }
}

impl Default for DocumentUrl {
    fn default() -> Self {
        Self::from_str("data:text/css;charset=utf-8;base64,").unwrap()
    }
}
impl FromStr for DocumentUrl {
    type Err = <Url as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let base_url = ServoArc::new(Url::parse(s)?);
        Ok(Self { base_url })
    }
}
impl From<Url> for DocumentUrl {
    fn from(base_url: Url) -> Self {
        Self {
            base_url: ServoArc::new(base_url),
        }
    }
}
impl From<ServoArc<Url>> for DocumentUrl {
    fn from(base_url: ServoArc<Url>) -> Self {
        Self { base_url }
    }
}
impl Deref for DocumentUrl {
    type Target = Url;
    fn deref(&self) -> &Self::Target {
        &self.base_url
    }
}
