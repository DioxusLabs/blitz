//! Conversion between winit's and Blitz's representations of a drag

use std::ops::ControlFlow;
use std::path::PathBuf;

use blitz_traits::events::{BlitzDataTransfer, BlitzDragOperation};
use winit::data_transfer::{DataTransfer, TransferType, TypeHint, TypedData};
use winit::event_loop::DndAction;

pub(crate) fn blitz_drag_operation_to_winit_dnd_action(
    data: BlitzDragOperation,
) -> Option<DndAction> {
    match data {
        BlitzDragOperation::None => None,
        BlitzDragOperation::Copy => Some(DndAction::Copy),
        BlitzDragOperation::Move => Some(DndAction::Move),
        BlitzDragOperation::Link => Some(DndAction::Link),
    }
}

pub(crate) fn winit_dnd_action_to_blitz_dnd_operation(
    action: Option<DndAction>,
) -> BlitzDragOperation {
    match action {
        Some(DndAction::Move) => BlitzDragOperation::Move,
        Some(DndAction::Copy) => BlitzDragOperation::Copy,
        Some(DndAction::Link) => BlitzDragOperation::Link,
        _ => BlitzDragOperation::None,
    }
}

/// The format that dragged files and links are exposed as by the drag source
pub(crate) const URI_LIST_FORMAT: &str = "text/uri-list";

/// The format of the data that a type of dragged data is exposed as, for the types
/// whose data is text
fn transfer_type_format(type_: &dyn TransferType) -> Option<&'static str> {
    match type_.hint()? {
        TypeHint::Plaintext => Some("text/plain"),
        TypeHint::UriList => Some(URI_LIST_FORMAT),
        TypeHint::Html => Some("text/html"),
        TypeHint::Rtf => Some("text/rtf"),
        _ => None,
    }
}

/// Call `callback` with each type of data a drag carries that Blitz can read, along
/// with the format it is exposed as
pub(crate) fn for_each_readable_type(
    data_transfer: &dyn DataTransfer,
    mut callback: impl FnMut(&dyn TransferType, &'static str),
) {
    data_transfer.for_each_available_type(&mut |type_| {
        if let Some(format) = transfer_type_format(type_) {
            callback(type_, format);
        }
        ControlFlow::Continue(())
    });
}

/// Add data received from the drag source to a drag's data
pub(crate) fn store_typed_data(data_transfer: &mut BlitzDataTransfer, value: &dyn TypedData) {
    let Some(format) = transfer_type_format(value.type_()) else {
        return;
    };

    if format == URI_LIST_FORMAT {
        let Ok(uris) = value.try_as_uris() else {
            return;
        };
        // Files are dragged as a list of `file://` URIs. They are exposed as files, which
        // leaves the URIs that are links.
        let mut links = Vec::new();
        for uri in uris {
            match file_uri_to_path(&uri) {
                Some(path) => data_transfer.add_file(path),
                None => links.push(uri),
            }
        }
        if !links.is_empty() {
            data_transfer.set_data_unchecked(format, links.join("\r\n"));
        }
    } else if let Ok(data) = value.try_as_string() {
        data_transfer.set_data_unchecked(format, data);
    }
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    return url.to_file_path().ok();
    // `Url::to_file_path` does not exist on wasm32-unknown-unknown.
    #[cfg(target_arch = "wasm32")]
    return Some(PathBuf::from(url.path()));
}
