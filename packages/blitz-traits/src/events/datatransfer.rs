use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use bitflags::bitflags;

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
    fn as_flag(self) -> BlitzDragOperations {
        match self {
            Self::None => BlitzDragOperations::NONE,
            Self::Copy => BlitzDragOperations::COPY,
            Self::Move => BlitzDragOperations::MOVE,
            Self::Link => BlitzDragOperations::LINK,
        }
    }

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

/// The data carried by a drag.
///
/// One of these is shared by every event of a drag. What an event handler can do with
/// it depends on the [mode](BlitzDragDataStoreMode) the event is dispatched in.
#[derive(Debug, Default)]
pub struct BlitzDataTransfer {
    /// In the order they were added, with at most one string per format
    items: Vec<DataTransferItem>,
    /// `None` is "uninitialized": every operation is allowed
    effect_allowed: Option<BlitzDragOperations>,
    drop_effect: BlitzDragOperation,
    mode: BlitzDragDataStoreMode,
}

/// A string or a file that a drag carries
#[derive(Debug)]
pub struct DataTransferItem {
    /// The MIME type of the data, in lowercase
    format: String,
    data: DataTransferItemData,
}

#[derive(Debug)]
enum DataTransferItemData {
    /// A string that another application offers, which is not fetched until the drop
    Unfetched,
    Text(String),
    File(PathBuf),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DataTransferItemKind {
    Text,
    File,
}

impl DataTransferItem {
    /// The MIME type of the data, in lowercase
    pub fn format(&self) -> &str {
        &self.format
    }

    pub fn kind(&self) -> DataTransferItemKind {
        match self.data {
            DataTransferItemData::Unfetched | DataTransferItemData::Text(_) => {
                DataTransferItemKind::Text
            }
            DataTransferItemData::File(_) => DataTransferItemKind::File,
        }
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> BlitzDragDataStoreMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: BlitzDragDataStoreMode) {
        self.mode = mode;
    }

    /// What the drag carries. The data of an item is read with [`get_data`](Self::get_data)
    /// or [`files`](Self::files).
    pub fn items(&self) -> &[DataTransferItem] {
        &self.items
    }

    /// The formats the drag carries strings in, followed by "Files" if it carries files
    pub fn types(&self) -> Vec<String> {
        let mut types = Vec::new();
        let mut has_files = false;
        for item in &self.items {
            match item.kind() {
                DataTransferItemKind::Text => types.push(item.format.clone()),
                DataTransferItemKind::File => has_files = true,
            }
        }
        if has_files {
            types.push("Files".to_string());
        }
        types
    }

    /// The files being dragged. Empty until the drop.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        let is_protected = self.mode == BlitzDragDataStoreMode::Protected;
        self.items.iter().filter_map(move |item| match &item.data {
            DataTransferItemData::File(path) if !is_protected => Some(path.as_path()),
            _ => None,
        })
    }

    /// Add a string that the drag source offers but that is not available yet. For the
    /// shell, not for event handlers.
    pub fn add_unfetched(&mut self, format: &str) {
        if self.text_item_mut(format).is_none() {
            self.items.push(DataTransferItem {
                format: format.to_string(),
                data: DataTransferItemData::Unfetched,
            });
        }
    }

    /// Add a file. For the shell, not for event handlers.
    pub fn add_file(&mut self, path: PathBuf) {
        self.items.push(DataTransferItem {
            // The MIME type of a file is not known
            format: "application/octet-stream".to_string(),
            data: DataTransferItemData::File(path),
        });
    }

    fn text_item_mut(&mut self, format: &str) -> Option<&mut DataTransferItem> {
        self.items
            .iter_mut()
            .find(|item| item.kind() == DataTransferItemKind::Text && item.format == format)
    }

    pub fn effect_allowed(&self) -> Option<BlitzDragOperations> {
        self.effect_allowed
    }

    /// The `effectAllowed` attribute: "uninitialized" or one of the values
    /// [`BlitzDragOperations::as_str`] returns
    pub fn effect_allowed_str(&self) -> &'static str {
        self.effect_allowed
            .map(BlitzDragOperations::as_str)
            .unwrap_or("uninitialized")
    }

    /// Set the `effectAllowed` attribute. Ignored outside of `dragstart` and for an
    /// unknown value.
    pub fn set_effect_allowed_str(&mut self, effect: &str) {
        if self.mode == BlitzDragDataStoreMode::ReadWrite
            && let Ok(op) = BlitzDragOperations::from_str_opt(effect)
        {
            self.effect_allowed = op;
        }
    }

    pub fn drop_effect(&self) -> BlitzDragOperation {
        self.drop_effect
    }

    pub fn set_drop_effect(&mut self, effect: BlitzDragOperation) {
        self.drop_effect = effect;
    }

    /// Set the `dropEffect` attribute. Ignored for an unknown value.
    pub fn set_drop_effect_str(&mut self, effect: &str) {
        if let Ok(op) = effect.parse() {
            self.drop_effect = op;
        }
    }

    /// The `dropEffect` a `dragenter` or `dragover` event starts with: the first
    /// operation the source allows, preferring `preferred` if it is one of them.
    /// <https://html.spec.whatwg.org/multipage/dnd.html#fire-a-dnd-event>
    pub fn default_drop_effect(&self, preferred: BlitzDragOperation) -> BlitzDragOperation {
        let Some(allowed) = self.effect_allowed else {
            return match preferred {
                BlitzDragOperation::None => BlitzDragOperation::Copy,
                preferred => preferred,
            };
        };
        [
            preferred,
            BlitzDragOperation::Copy,
            BlitzDragOperation::Link,
            BlitzDragOperation::Move,
        ]
        .into_iter()
        .find(|op| *op != BlitzDragOperation::None && allowed.contains(op.as_flag()))
        .unwrap_or_default()
    }

    /// The operation a drop would perform: `dropEffect` if the source allows it
    pub fn allowed_drop_effect(&self) -> BlitzDragOperation {
        match self.effect_allowed {
            Some(allowed) if !allowed.contains(self.drop_effect.as_flag()) => {
                BlitzDragOperation::None
            }
            _ => self.drop_effect,
        }
    }

    /// Read data regardless of the mode. For the shell, not for event handlers.
    pub fn data_unchecked(&self, format: &str) -> Option<&str> {
        self.items.iter().find_map(|item| match &item.data {
            DataTransferItemData::Text(data) if item.format == format => Some(data.as_str()),
            _ => None,
        })
    }

    /// Add data regardless of the mode. For the shell, not for event handlers.
    pub fn set_data_unchecked(&mut self, format: &str, data: String) {
        let data = DataTransferItemData::Text(data);
        match self.text_item_mut(format) {
            Some(item) => item.data = data,
            None => self.items.push(DataTransferItem {
                format: format.to_string(),
                data,
            }),
        }
    }

    pub fn get_data(&self, format: &str) -> Option<String> {
        if self.mode == BlitzDragDataStoreMode::Protected {
            return None;
        }
        self.data_unchecked(&normalize_format(format))
            .map(str::to_string)
    }

    pub fn set_data(&mut self, format: &str, data: &str) -> Result<(), String> {
        if self.mode != BlitzDragDataStoreMode::ReadWrite {
            return Err("setData is only allowed during dragstart".into());
        }
        self.set_data_unchecked(&normalize_format(format), data.to_string());
        Ok(())
    }

    pub fn clear_data(&mut self, format: Option<&str>) -> Result<(), String> {
        if self.mode != BlitzDragDataStoreMode::ReadWrite {
            return Err("clearData is only allowed during dragstart".into());
        }

        // Files are kept
        // https://developer.mozilla.org/en-US/docs/Web/API/DataTransfer/clearData
        let format = format.map(normalize_format);
        self.items.retain(|item| {
            item.kind() == DataTransferItemKind::File
                || format.as_ref().is_some_and(|format| item.format != *format)
        });
        Ok(())
    }
}
