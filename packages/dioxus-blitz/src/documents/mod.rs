mod dioxus_document;
mod html_document;

use blitz_dom::Document;

pub(crate) use dioxus_document::DioxusDocument;
pub(crate) use html_document::HtmlDocument;

pub(crate) trait DocumentLike: AsRef<Document> + AsMut<Document> + Into<Document> {}

impl DocumentLike for Document {}
