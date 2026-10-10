use std::any::Any;
use std::pin::Pin;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

use bitflags::bitflags;
use bytes::Bytes;

use crate::NodeId;

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct BlitzDragOperations: u8 {
        const NONE = 0;
        const COPY = 1 << 0;
        const MOVE = 1 << 1;
        const LINK = 1 << 2;
    }
}

impl BlitzDragOperations {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NONE => "none",
            Self::COPY => "copy",
            Self::MOVE => "move",
            Self::LINK => "link",
            x if x == (Self::COPY | Self::MOVE) => "copyMove",
            x if x == (Self::COPY | Self::LINK) => "copyLink",
            x if x == (Self::MOVE | Self::LINK) => "linkMove",
            x if x == (Self::COPY | Self::MOVE | Self::LINK) => "all",
            _ => "uninitialized",
        }
    }
    pub fn from_str_opt(s: &str) -> Result<Option<Self>, ParseDragOperationsError> {
        match s.trim() {
            "none" => Ok(Some(Self::NONE)),
            "copy" => Ok(Some(Self::COPY)),
            "move" => Ok(Some(Self::MOVE)),
            "link" => Ok(Some(Self::LINK)),
            "copyMove" => Ok(Some(Self::COPY | Self::MOVE)),
            "copyLink" => Ok(Some(Self::COPY | Self::LINK)),
            "linkMove" => Ok(Some(Self::MOVE | Self::LINK)),
            "all" => Ok(Some(Self::COPY | Self::MOVE | Self::LINK)),
            "uninitialized" => Ok(None),
            _ => Err(ParseDragOperationsError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseDragOperationsError;

impl std::fmt::Display for ParseDragOperationsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid drag effect string")
    }
}

impl std::error::Error for ParseDragOperationsError {}

impl std::fmt::Display for BlitzDragOperations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum BlitzDragOperation {
    #[default]
    None,
    Copy,
    Move,
    Link,
}

impl BlitzDragOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }
}

impl std::fmt::Display for BlitzDragOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BlitzDragOperation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "none" => Ok(Self::None),
            "copy" => Ok(Self::Copy),
            "move" => Ok(Self::Move),
            "link" => Ok(Self::Link),
            _ => Err(()),
        }
    }
}

// https://html.spec.whatwg.org/multipage/dnd.html#drag-data-store-mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlitzDragDataStoreMode {
    /// getData/setData/clearData forbidden (dragenter/dragover/dragleave/drag/dragend)
    #[default]
    Protected,
    /// getData allowed, setData/clearData forbidden (drop)
    ReadOnly,
    /// getData/setData/clearData allowed (dragstart)
    ReadWrite,
}

impl BlitzDragDataStoreMode {
    pub fn can_write(&self) -> bool {
        matches!(self, Self::ReadWrite)
    }

    pub fn is_protected(&self) -> bool {
        matches!(self, Self::Protected)
    }
}

pub type BlitzFileType = Arc<dyn BlitzFileTrait>;

pub type BlitzFileError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait BlitzFileTrait: Any + Send + Sync + std::fmt::Debug {
    fn name(&self) -> String;
    fn size(&self) -> u64;
    fn last_modified(&self) -> u64;
    fn path(&self) -> PathBuf;
    fn content_type(&self) -> Option<String>;

    fn read_bytes(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Bytes, BlitzFileError>> + Send + 'static>>;

    fn byte_stream(
        &self,
    ) -> Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, BlitzFileError>> + Send + 'static>>;

    fn read_string(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, BlitzFileError>> + Send + 'static>>;

    fn inner(&self) -> &dyn std::any::Any;
}

pub trait BlitzDataTransferItemsTrait: Any + Send + Sync + std::fmt::Debug {
    fn is_empty(&self) -> bool;
    fn files(&self) -> Box<dyn Iterator<Item = BlitzFileType> + '_> {
        Box::new(std::iter::empty())
    }
    fn get_data(&self, format: &str) -> Option<String> {
        let _ = format;
        None
    }
    fn set_data(&mut self, format: &str, data: &str) -> Result<(), String> {
        let _ = format;
        let _ = data;
        Err("setData is not supported on received data transfer".into())
    }
    fn clear_all(&mut self) {}
    fn clear_format(&mut self, format: &str) {
        let _ = format;
    }
    fn types(&self) -> Vec<String> {
        Vec::new()
    }
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug)]
pub struct BlitzDataTransfer {
    pub items: Box<dyn BlitzDataTransferItemsTrait>,
    pub effect_allowed: Option<BlitzDragOperations>,
    pub drop_effect: BlitzDragOperation,
    pub mode: BlitzDragDataStoreMode,
}

impl PartialEq for BlitzDataTransfer {
    fn eq(&self, other: &Self) -> bool {
        self.effect_allowed == other.effect_allowed
            && self.drop_effect == other.drop_effect
            && self.mode == other.mode
    }
}

/// legacy
fn normalize_format(format: &str) -> String {
    match format.to_ascii_lowercase().as_str() {
        "text" => "text/plain".to_string(),
        "url" => "text/uri-list".to_string(),
        other => other.to_string(),
    }
}

impl BlitzDataTransfer {
    pub fn with_writable(mut self) -> Self {
        self.mode = BlitzDragDataStoreMode::ReadWrite;
        self
    }

    pub fn protected(&mut self) {
        self.mode = BlitzDragDataStoreMode::Protected;
    }

    pub fn readable(&mut self) {
        self.mode = BlitzDragDataStoreMode::ReadOnly;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn files(&self) -> impl Iterator<Item = BlitzFileType> {
        self.items.files()
    }

    pub fn effect_allowed(&self) -> String {
        self.effect_allowed
            .map(|v| v.to_string())
            .unwrap_or("uninitialized".to_string())
    }

    pub fn set_effect_allowed(&mut self, effect: &str) {
        if self.mode.can_write()
            && let Ok(op) = BlitzDragOperations::from_str_opt(effect)
        {
            self.effect_allowed = op;
        }
    }

    pub fn drop_effect(&self) -> String {
        self.drop_effect.to_string()
    }

    pub fn set_drop_effect(&mut self, effect: &str) {
        if let Ok(op) = effect.parse() {
            self.drop_effect = op;
        }
    }

    pub fn get_data_unchecked(&self, format: &str) -> Option<String> {
        let format = normalize_format(format);
        self.items.get_data(&format)
    }

    pub fn get_data(&self, format: &str) -> Option<String> {
        if self.mode == BlitzDragDataStoreMode::Protected {
            return None;
        }
        let format = normalize_format(format);
        self.items.get_data(&format)
    }

    pub fn set_data(&mut self, format: &str, data: &str) -> Result<(), String> {
        if !self.mode.can_write() {
            return Err("setData is only allowed during dragstart".into());
        }
        let format = normalize_format(format);
        self.items.set_data(&format, data)
    }

    pub fn clear_data(&mut self, format: Option<&str>) -> Result<(), String> {
        if !self.mode.can_write() {
            return Err("clearData is only allowed during dragstart".into());
        }

        match format {
            Some(fmt) => {
                let fmt = normalize_format(fmt);
                self.items.clear_format(&fmt)
            }
            // https://developer.mozilla.org/en-US/docs/Web/API/DataTransfer/clearData
            None => self.items.clear_all(),
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum BlitzDataTransferSource {
    Internal(Option<NodeId>),
    External,
}

impl BlitzDataTransferSource {
    pub fn get_node_id(&self) -> Option<NodeId> {
        match self {
            BlitzDataTransferSource::Internal(node) => *node,
            BlitzDataTransferSource::External => None,
        }
    }
}

impl From<Option<NodeId>> for BlitzDataTransferSource {
    fn from(value: Option<NodeId>) -> Self {
        Self::Internal(value)
    }
}
