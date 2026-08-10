use std::any::Any;
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use blitz_traits::events::{
    BlitzDataTransferArc, BlitzDataTransferItemsTrait, BlitzDragOperation, BlitzDragOperations,
    BlitzFileError, BlitzFileTrait, BlitzFileType,
};

use bytes::Bytes;
use futures_util::Stream;
use winit::data_transfer::{
    DataTransfer, DataTransferSend, SendData, TransferType, TypeHint, TypedData,
};

use winit::event_loop::DndAction;

pub(crate) fn blitz_drag_operation_to_winit_dnd_action(
    data: &BlitzDragOperation,
) -> Option<DndAction> {
    match data {
        BlitzDragOperation::None => None,
        BlitzDragOperation::Copy => Some(DndAction::Copy),
        BlitzDragOperation::Move => Some(DndAction::Move),
        BlitzDragOperation::Link => Some(DndAction::Link),
    }
}

pub(crate) fn blitz_drag_operations_to_winit_dnd_action(
    data: &Option<BlitzDragOperations>,
) -> Vec<DndAction> {
    match data {
        Some(ops) => {
            let mut actions = Vec::new();
            if ops.contains(BlitzDragOperations::COPY) {
                actions.push(DndAction::Copy);
            }
            if ops.contains(BlitzDragOperations::MOVE) {
                actions.push(DndAction::Move);
            }
            if ops.contains(BlitzDragOperations::LINK) {
                actions.push(DndAction::Link);
            }
            actions
        }

        None => vec![
            DndAction::Move,
            DndAction::Copy,
            DndAction::Link,
            DndAction::Ask,
            DndAction::Private,
        ],
    }
}

pub(crate) fn winit_dnd_action_to_blitz_dnd_operation(
    action: Option<DndAction>,
) -> BlitzDragOperation {
    match action {
        Some(DndAction::Move) => BlitzDragOperation::Move,
        Some(DndAction::Copy) => BlitzDragOperation::Copy,
        Some(DndAction::Link) => BlitzDragOperation::Link,
        Some(DndAction::Ask) => BlitzDragOperation::None,
        Some(DndAction::Private) => BlitzDragOperation::None,
        Some(action) => unreachable!("unsupported DndAction variant: {action:?}"),
        None => BlitzDragOperation::None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BlitzTransferType(String);

impl TransferType for BlitzTransferType {
    fn hint(&self) -> Option<TypeHint> {
        match self.0.as_str() {
            "text/plain" => Some(TypeHint::Plaintext),
            "text/uri-list" => Some(TypeHint::UriList),
            "text/html" => Some(TypeHint::Html),
            "text/rtf" | "application/rtf" => Some(TypeHint::Rtf),
            ct if ct.starts_with("audio/") => Some(TypeHint::Audio {
                extension_hint: audio_mime_to_static_ext(&ct[6..]),
            }),
            ct if ct.starts_with("image/") => Some(TypeHint::Image {
                extension_hint: image_mime_to_static_ext(&ct[6..]),
            }),
            _ => None,
        }
    }

    fn matches(&self, other: &dyn TransferType) -> bool {
        other.cast_ref::<Self>().is_some_and(|o| o.0 == self.0)
    }
}

fn audio_mime_to_static_ext(ext: &str) -> Option<&'static str> {
    if ext.is_empty() {
        return None;
    }

    let normalized = match ext {
        "mpeg" => "mp3",
        "x-flac" => "flac",
        "x-wav" | "wave" | "vnd.wave" => "wav",
        "x-aiff" => "aif",
        "x-m4a" => "m4a",
        "3gpp" => "3ga",
        "vnd.dlna.adts" => "aac",
        "basic" => "au",
        "x-midi" => "mid",
        "vorbis" => "ogg",
        other => other,
    };

    Some(Box::leak(normalized.to_string().into_boxed_str()))
}

fn image_mime_to_static_ext(ext: &str) -> Option<&'static str> {
    if ext.is_empty() {
        return None;
    }

    let normalized = match ext {
        "jpeg" => "jpg",
        "svg+xml" => "svg",
        "tiff" => "tif",
        "heif" => "heic",
        "x-portable-bitmap" => "pbm",
        "x-portable-graymap" => "pgm",
        "x-portable-pixmap" => "ppm",
        "x-portable-anymap" => "pnm",
        other => other,
    };

    Some(Box::leak(normalized.to_string().into_boxed_str()))
}

#[derive(Debug)]
pub(crate) struct WinitDataTransfer {
    data: BlitzDataTransferArc,
    types: Vec<BlitzTransferType>,
}

impl WinitDataTransfer {
    pub fn new(data: BlitzDataTransferArc) -> Self {
        let types = data
            .borrow()
            .items
            .types()
            .into_iter()
            .map(BlitzTransferType)
            .collect();

        Self { data, types }
    }
}

impl DataTransfer for WinitDataTransfer {
    fn for_each_available_type<'this>(
        &'this self,
        func: &'_ mut dyn FnMut(&'this dyn TransferType) -> ControlFlow<()>,
    ) {
        for hint in &self.types {
            if let ControlFlow::Break(()) = func(hint) {
                break;
            }
        }
    }
}

impl DataTransferSend for WinitDataTransfer {
    fn data_for_type(&self, type_: &dyn TransferType) -> Option<SendData> {
        let format = type_
            .hint()
            .map(|hint| winit_hint_to_mimetype(&hint))
            .or_else(|| type_.cast_ref::<BlitzTransferType>().map(|t| t.0.clone()))?;
        let transfer = self.data.borrow();
        let data = transfer.get_data_unchecked(&format)?;

        if format == "text/uri-list" {
            let uris = data
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(String::from)
                .collect();
            Some(SendData::Uris(uris))
        } else {
            Some(SendData::String(data))
        }
    }
}

#[derive(Debug)]
pub struct WinitDataTransferItems {
    items: Vec<Arc<dyn TypedData>>,
}

impl WinitDataTransferItems {
    pub(crate) fn new(items: Vec<Arc<dyn TypedData>>) -> Self {
        Self { items }
    }
    pub(crate) fn push(&mut self, item: Arc<dyn TypedData>) {
        self.items.push(item);
    }
    pub fn items(&self) -> &Vec<Arc<dyn TypedData>> {
        &self.items
    }
}

impl BlitzDataTransferItemsTrait for WinitDataTransferItems {
    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn files(&self) -> Box<dyn Iterator<Item = BlitzFileType> + '_> {
        Box::new(
            self.items
                .iter()
                .flat_map(|item| {
                    let hint = item.type_().hint();
                    item.try_as_file_paths()
                        .into_iter()
                        .flatten()
                        .map(move |p| (p, hint))
                })
                .scan(HashSet::new(), |seen, (p, hint)| {
                    Some(seen.insert(p.clone()).then_some((p, hint)))
                })
                .flatten()
                .map(|(p, hint)| Arc::new(WinitBlitzFile::new(p, hint)) as BlitzFileType),
        )
    }

    fn get_data(&self, format: &str) -> Option<String> {
        self.items.iter().find_map(|item| {
            let hint = item.type_().hint()?;
            let mime = winit_hint_to_mimetype(&hint);
            (mime == format).then(|| item.try_as_string().ok())?
        })
    }

    fn types(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        self.items
            .iter()
            .filter_map(|item| {
                item.type_()
                    .hint()
                    .map(|hint| winit_hint_to_mimetype(&hint))
            })
            .filter(|mime| seen.insert(mime.clone()))
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }
}

#[derive(Debug)]
struct WinitBlitzFile {
    path: PathBuf,
    content_type: Option<String>,
}

fn winit_hint_to_mimetype(hint: &TypeHint) -> String {
    match hint {
        TypeHint::Plaintext => "text/plain".to_string(),
        TypeHint::UriList => "text/uri-list".to_string(),
        TypeHint::Html => "text/html".to_string(),
        TypeHint::Rtf => "application/rtf".to_string(),
        TypeHint::Audio { extension_hint } => extension_hint
            .map(|ext| format!("audio/{ext}"))
            .unwrap_or_else(|| "audio".to_string()),
        TypeHint::Image { extension_hint } => extension_hint
            .map(|ext| format!("image/{ext}"))
            .unwrap_or_else(|| "image".to_string()),
        _ => unreachable!("unsupported TypeHint variant: {hint:?}"),
    }
}
impl WinitBlitzFile {
    fn new(path: PathBuf, hint: Option<TypeHint>) -> Self {
        Self {
            path,
            content_type: hint.as_ref().map(winit_hint_to_mimetype),
        }
    }
}

impl BlitzFileTrait for WinitBlitzFile {
    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn size(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    fn last_modified(&self) -> u64 {
        std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn content_type(&self) -> Option<String> {
        self.content_type.clone()
    }

    fn read_bytes(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Bytes, BlitzFileError>> + Send + 'static>> {
        let path = self.path.clone();
        Box::pin(async move {
            std::fs::read(&path)
                .map(Bytes::from)
                .map_err(|e| Box::new(e) as BlitzFileError)
        })
    }

    fn byte_stream(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, BlitzFileError>> + Send + 'static>> {
        use futures_util::StreamExt;
        use std::io::Read;

        let path = self.path.clone();

        let stream = futures_util::stream::unfold(None::<std::fs::File>, move |file_opt| {
            let path = path.clone();
            async move {
                let mut file = match file_opt {
                    Some(f) => f,
                    None => match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(e) => return Some((Err(Box::new(e) as BlitzFileError), None)),
                    },
                };

                let mut buf = vec![0u8; 64 * 1024];
                match file.read(&mut buf) {
                    Ok(0) => None,
                    Ok(n) => {
                        buf.truncate(n);
                        Some((Ok(Bytes::from(buf)), Some(file)))
                    }
                    Err(e) => Some((Err(Box::new(e) as BlitzFileError), None)),
                }
            }
        });

        stream.boxed()
    }

    fn read_string(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, BlitzFileError>> + Send + 'static>> {
        let path = self.path.clone();
        Box::pin(async move {
            std::fs::read_to_string(&path).map_err(|e| Box::new(e) as BlitzFileError)
        })
    }

    fn inner(&self) -> &dyn std::any::Any {
        self
    }
}
