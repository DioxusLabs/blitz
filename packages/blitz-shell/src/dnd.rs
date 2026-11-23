//! Drags over the window, and conversion between winit's and Blitz's representations
//! of a drag

use std::ops::ControlFlow;
use std::path::PathBuf;

use blitz_dom::Document;
use blitz_traits::events::{
    BlitzDataTransfer, BlitzDragEvent, BlitzDragOperation, BlitzDragOperations, MouseEventButtons,
    PointerCoords, UiEvent,
};
use keyboard_types::Modifiers;
use winit::data_transfer::{
    DataTransfer, DataTransferId, DataTransferSend, DataTransferSendBuilder, SendData,
    TransferType, TypeHint, TypedData,
};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, DndAction};

/// A drag that is over the window
pub(crate) struct IncomingDrag {
    id: DataTransferId,
    /// The event dispatched to the document each time the drag moves
    event: BlitzDragEvent,
    /// The requests for dragged data that are yet to be answered: for the files when
    /// the drag enters the window, and for the strings when it is dropped
    pending_data: Vec<AsyncRequestSerial>,
    /// The operation the drag source preferred when the drag last moved, if that was
    /// while its files were not known
    held_move: Option<Option<DndAction>>,
    /// Whether the drag has been dropped, and the drop is waiting for its data
    is_dropped: bool,
    /// Whether this is the drag that the document started, sharing its data
    is_internal: bool,
}

impl IncomingDrag {
    /// Track a drag from another application that has entered the window
    pub(crate) fn new(
        event_loop: &dyn ActiveEventLoop,
        id: DataTransferId,
        coords: PointerCoords,
        mods: Modifiers,
        buttons: MouseEventButtons,
    ) -> Self {
        // The formats of the strings being dragged are known for the whole drag,
        // and the strings are only fetched if the drag is dropped. Which files are
        // being dragged is only known from the list of URIs, so that is fetched now.
        let mut data_transfer = BlitzDataTransfer::new();
        let mut pending_data = Vec::new();
        if let Ok(source) = event_loop.data_transfer(id) {
            for_each_readable_type(&*source, |type_, format| {
                if format != URI_LIST_FORMAT {
                    data_transfer.add_unfetched(format);
                } else if pending_data.is_empty()
                    && let Ok(serial) = event_loop.fetch_data_transfer(id, type_)
                {
                    pending_data.push(serial);
                }
            });
        }

        // The OS is given an answer before the document has seen the drag. On
        // macOS a drag that is refused as it enters the frontmost application
        // can't be accepted later.
        if !pending_data.is_empty() {
            let _ = event_loop.set_valid_dnd_actions(id, &[DndAction::Copy]);
        }

        let mut event = BlitzDragEvent::new(coords, data_transfer);
        event.mods = mods;
        event.buttons = buttons;

        Self {
            id,
            event,
            pending_data,
            held_move: None,
            is_dropped: false,
            is_internal: false,
        }
    }

    /// Track the drag that the document started, which has entered the window.
    /// `event` is the event of that drag.
    pub(crate) fn internal(id: DataTransferId, event: BlitzDragEvent) -> Self {
        Self {
            id,
            event,
            pending_data: Vec::new(),
            held_move: None,
            is_dropped: false,
            is_internal: true,
        }
    }

    pub(crate) fn id(&self) -> DataTransferId {
        self.id
    }

    pub(crate) fn is_internal(&self) -> bool {
        self.is_internal
    }

    /// The event dispatched to the document for the drag
    pub(crate) fn into_event(self) -> BlitzDragEvent {
        self.event
    }

    /// Handle the drag moving over the window
    pub(crate) fn moved(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        doc: &mut dyn Document,
        coords: PointerCoords,
        mods: Modifiers,
        proposed_action: Option<DndAction>,
    ) {
        self.event.coords = coords;
        self.event.mods = mods;
        // The document is not told about a drag until it is known which files it carries
        if !self.pending_data.is_empty() {
            self.held_move = Some(proposed_action);
            return;
        }
        if self.is_internal {
            doc.handle_ui_event(UiEvent::Drag(self.event.clone()));
        }
        self.dispatch_drag_over(event_loop, doc, proposed_action);
    }

    /// Tell the document that the drag is over it, and the OS whether the document would
    /// accept a drop
    fn dispatch_drag_over(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        doc: &mut dyn Document,
        proposed_action: Option<DndAction>,
    ) {
        // The document replaces the operation the source prefers with the one a drop
        // would perform
        self.event
            .data_transfer_mut()
            .set_drop_effect(winit_dnd_action_to_blitz_dnd_operation(proposed_action));

        doc.handle_ui_event(UiEvent::DragOver(self.event.clone()));

        let drop_effect = self.event.data_transfer().drop_effect();
        let action = blitz_drag_operation_to_winit_dnd_action(drop_effect);
        let _ = event_loop.set_valid_dnd_actions(self.id, action.as_slice());
    }

    /// Handle the drag being dropped on the window. Returns the drag if it is not over.
    pub(crate) fn dropped(
        mut self,
        event_loop: &dyn ActiveEventLoop,
        doc: &mut dyn Document,
    ) -> Option<Self> {
        // A drag that the document was not told about has not been accepted
        if !self.pending_data.is_empty() {
            return None;
        }

        // The strings have to be requested now: the drag source may not keep them
        // available once the drop has been handled. The `drop` event is
        // dispatched once all of them have arrived.
        self.is_dropped = true;
        if !self.is_internal
            && let Ok(source) = event_loop.data_transfer(self.id)
        {
            for_each_readable_type(&*source, |type_, format| {
                if format != URI_LIST_FORMAT
                    && let Ok(serial) = event_loop.fetch_data_transfer(self.id, type_)
                {
                    self.pending_data.push(serial);
                }
            });
        }

        if !self.pending_data.is_empty() {
            return Some(self);
        }
        doc.handle_ui_event(UiEvent::Drop(self.event));
        None
    }

    /// Handle data that was requested from the drag source arriving. Returns the drag if
    /// it is not over.
    pub(crate) fn data_received(
        mut self,
        event_loop: &dyn ActiveEventLoop,
        doc: &mut dyn Document,
        serial: AsyncRequestSerial,
        value: &dyn TypedData,
    ) -> Option<Self> {
        let Some(idx) = self.pending_data.iter().position(|s| *s == serial) else {
            return Some(self);
        };
        self.pending_data.swap_remove(idx);
        store_typed_data(&mut self.event.data_transfer_mut(), value);

        if !self.pending_data.is_empty() {
            return Some(self);
        }
        if self.is_dropped {
            doc.handle_ui_event(UiEvent::Drop(self.event));
            return None;
        }
        if let Some(proposed_action) = self.held_move.take() {
            self.dispatch_drag_over(event_loop, doc, proposed_action);
        }
        Some(self)
    }

    /// Handle the drag leaving the window. Returns the drag if it is not over.
    pub(crate) fn left(self, doc: &mut dyn Document) -> Option<Self> {
        // A drag whose drop is waiting for its data has not left
        if self.is_dropped {
            return Some(self);
        }
        // The document was not told about a drag that is still waiting for its files
        if self.pending_data.is_empty() {
            doc.handle_ui_event(UiEvent::DragLeave(self.event));
        }
        None
    }
}

fn blitz_drag_operation_to_winit_dnd_action(data: BlitzDragOperation) -> Option<DndAction> {
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
const URI_LIST_FORMAT: &str = "text/uri-list";

/// The operations that the source of a drag allows a drop to perform
pub(crate) fn allowed_dnd_actions(data_transfer: &BlitzDataTransfer) -> Vec<DndAction> {
    let allowed = data_transfer
        .effect_allowed()
        .unwrap_or(BlitzDragOperations::all());
    [
        (BlitzDragOperations::COPY, DndAction::Copy),
        (BlitzDragOperations::MOVE, DndAction::Move),
        (BlitzDragOperations::LINK, DndAction::Link),
    ]
    .into_iter()
    .filter(|(operation, _)| allowed.contains(*operation))
    .map(|(_, action)| action)
    .collect()
}

/// The data of a drag that the document started, as offered to other applications.
/// This is the data as it is when the drag starts.
pub(crate) fn outgoing_drag_data(data_transfer: &BlitzDataTransfer) -> Box<dyn DataTransferSend> {
    let mut builder = DataTransferSendBuilder::new(());
    for (type_, format) in [
        (TypeHint::Plaintext, "text/plain"),
        (TypeHint::UriList, "text/uri-list"),
        (TypeHint::Html, "text/html"),
        (TypeHint::Rtf, "text/rtf"),
    ] {
        let Some(data) = data_transfer.data_unchecked(format) else {
            continue;
        };
        let data = if type_ == TypeHint::UriList {
            // Lines starting with `#` are comments
            let uris = data.lines().filter(|line| !line.starts_with('#'));
            SendData::Uris(uris.map(str::to_string).collect())
        } else {
            SendData::String(data.to_string())
        };
        builder.add_type(type_, move |_, _| Some(data.clone()));
    }
    builder.build()
}

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
fn for_each_readable_type(
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
fn store_typed_data(data_transfer: &mut BlitzDataTransfer, value: &dyn TypedData) {
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
