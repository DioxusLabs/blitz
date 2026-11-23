//! Drags over the window, and conversion between winit's and Blitz's representations
//! of a drag

use std::ops::ControlFlow;
use std::path::PathBuf;

use blitz_dom::Document;
use blitz_traits::events::{
    BlitzDataTransfer, BlitzDragEvent, BlitzDragOperation, MouseEventButtons, PointerCoords,
    UiEvent,
};
use keyboard_types::Modifiers;
use winit::data_transfer::{DataTransfer, DataTransferId, TransferType, TypeHint, TypedData};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, DndAction};

/// A drag from another application that is over the window
pub(crate) struct IncomingDrag {
    id: DataTransferId,
    /// The event dispatched to the document each time the drag moves
    event: BlitzDragEvent,
    /// The requests for dragged data that are yet to be answered: for the files when
    /// the drag enters the window, and for the strings when it is dropped
    pending_data: Vec<AsyncRequestSerial>,
    /// Set while the document is owed a `dragover` event, because the drag moved while
    /// its files were not known. Holds the operation the drag source proposed for it.
    deferred_drag_over: Option<Option<DndAction>>,
    /// Whether the drag has been dropped, and the drop is waiting for its data
    is_dropped: bool,
}

impl IncomingDrag {
    /// Track a drag that has entered the window
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
            for_each_readable_type(event_loop, &*source, |type_, format| {
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
            deferred_drag_over: None,
            is_dropped: false,
        }
    }

    pub(crate) fn id(&self) -> DataTransferId {
        self.id
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
            self.deferred_drag_over = Some(proposed_action);
            return;
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
        // The OS was told that a drag whose files are not known yet would be accepted,
        // so it can be dropped before the document has seen it. The document is asked
        // whether it accepts the drop once the files have arrived.
        if !self.pending_data.is_empty() {
            self.deferred_drag_over.get_or_insert(None);
        }

        // The strings have to be requested now: the drag source may not keep them
        // available once the drop has been handled. The `drop` event is
        // dispatched once all of them have arrived.
        self.is_dropped = true;
        if let Ok(source) = event_loop.data_transfer(self.id) {
            for_each_readable_type(event_loop, &*source, |type_, format| {
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
        if let Some(proposed_action) = self.deferred_drag_over.take() {
            self.dispatch_drag_over(event_loop, doc, proposed_action);
        }
        if self.is_dropped {
            doc.handle_ui_event(UiEvent::Drop(self.event));
            return None;
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

fn winit_dnd_action_to_blitz_dnd_operation(action: Option<DndAction>) -> BlitzDragOperation {
    match action {
        Some(DndAction::Move) => BlitzDragOperation::Move,
        Some(DndAction::Copy) => BlitzDragOperation::Copy,
        Some(DndAction::Link) => BlitzDragOperation::Link,
        _ => BlitzDragOperation::None,
    }
}

/// The format that dragged files and links are exposed as by the drag source
const URI_LIST_FORMAT: &str = "text/uri-list";

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

/// Whether winit can read dragged data of a type as a string
fn is_readable_type(event_loop: &dyn ActiveEventLoop, type_: &dyn TransferType) -> bool {
    // winit can't read RTF on X11
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    if type_.hint() == Some(TypeHint::Rtf) {
        use winit::platform::x11::ActiveEventLoopExtX11;
        return !event_loop.is_x11();
    }

    let _ = (event_loop, type_);
    true
}

/// Call `callback` with each type of data a drag carries that Blitz can read, along
/// with the format it is exposed as
fn for_each_readable_type(
    event_loop: &dyn ActiveEventLoop,
    data_transfer: &dyn DataTransfer,
    mut callback: impl FnMut(&dyn TransferType, &'static str),
) {
    data_transfer.for_each_available_type(&mut |type_| {
        if let Some(format) = transfer_type_format(type_)
            && is_readable_type(event_loop, type_)
        {
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
