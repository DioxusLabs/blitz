//! Abstractions of networking so that custom networking implementations can be provided

pub use bytes::Bytes;
pub use http::{self, HeaderMap, Method};
use serde::{
    Serialize,
    ser::{SerializeSeq, SerializeTuple},
};
use std::{ops::Deref, path::PathBuf, sync::Arc};
pub use url::Url;

pub type SharedProvider<D> = Arc<dyn NetProvider<D>>;
pub type BoxedHandler = Box<dyn NetHandler>;
pub type SharedCallback<D> = Arc<dyn NetCallback<D>>;

/// A type that fetches resources for a Document.
///
/// This may be over the network via http(s), via the filesystem, or some other method.
pub trait NetProvider<Data>: Send + Sync + 'static {
    fn fetch(&self, doc_id: usize, request: Request, handler: BoxedHandler);
}

/// A type that parses raw bytes from a network request into a Data and then calls
/// the NetCallack with the result.
pub trait NetHandler: Send + Sync + 'static {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes);
}

/// A type which accepts the parsed result of a network request and sends it back to the Document
/// (or does arbitrary things with it)
pub trait NetCallback<Data>: Send + Sync + 'static {
    fn call(&self, doc_id: usize, result: Result<Data, Option<String>>);
}

impl<D, F: Fn(usize, Result<D, Option<String>>) + Send + Sync + 'static> NetCallback<D> for F {
    fn call(&self, doc_id: usize, result: Result<D, Option<String>>) {
        self(doc_id, result)
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
/// A request type loosely representing <https://fetch.spec.whatwg.org/#requests>
pub struct Request {
    pub url: Url,
    pub method: Method,
    pub content_type: String,
    pub headers: HeaderMap,
    pub body: Body,
}
impl Request {
    /// A get request to the specified Url and an empty body
    pub fn get(url: Url) -> Self {
        Self {
            url,
            method: Method::GET,
            content_type: String::new(),
            headers: HeaderMap::new(),
            body: Body::Empty,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    Bytes(Bytes),
    Form(FormData),
    Empty,
}

/// A list of form entries used for form submission
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormData(pub Vec<Entry>);
impl FormData {
    /// Creates a new empty FormData
    pub fn new() -> Self {
        FormData(Vec::new())
    }
}
impl Serialize for FormData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq_serializer = serializer.serialize_seq(Some(self.len()))?;
        for entry in &self.0 {
            seq_serializer.serialize_element(entry)?;
        }
        seq_serializer.end()
    }
}
impl Deref for FormData {
    type Target = Vec<Entry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A single form entry consisting of a name and value
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub value: EntryValue,
}
impl Serialize for Entry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serializer = serializer.serialize_tuple(2)?;
        serializer.serialize_element(&self.name)?;
        match &self.value {
            EntryValue::String(s) => serializer.serialize_element(s)?,
            EntryValue::File(p) => serializer.serialize_element(p.to_str().unwrap_or_default())?,
            EntryValue::EmptyFile => serializer.serialize_element("")?,
        }
        serializer.end()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryValue {
    String(String),
    File(PathBuf),
    EmptyFile,
}
impl AsRef<str> for EntryValue {
    fn as_ref(&self) -> &str {
        match self {
            EntryValue::String(s) => s,
            EntryValue::File(p) => p.to_str().unwrap_or_default(),
            EntryValue::EmptyFile => "",
        }
    }
}

impl From<&str> for EntryValue {
    fn from(value: &str) -> Self {
        EntryValue::String(value.to_string())
    }
}
impl From<PathBuf> for EntryValue {
    fn from(value: PathBuf) -> Self {
        EntryValue::File(value)
    }
}

/// A default noop NetProvider
#[derive(Default)]
pub struct DummyNetProvider;
impl<D: Send + Sync + 'static> NetProvider<D> for DummyNetProvider {
    fn fetch(&self, _doc_id: usize, _request: Request, _handler: BoxedHandler) {}
}

/// A default noop NetCallback
#[derive(Default)]
pub struct DummyNetCallback;
impl<D: Send + Sync + 'static> NetCallback<D> for DummyNetCallback {
    fn call(&self, _doc_id: usize, _result: Result<D, Option<String>>) {}
}
