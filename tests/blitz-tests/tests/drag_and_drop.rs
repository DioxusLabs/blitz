//! A drag over a document fires `dragenter`/`dragover`/`dragleave`/`drop` at the node
//! under it, a drop is accepted by cancelling `dragover`, and a sub-document takes
//! part in a drag over its host.

use blitz_dom::{
    BaseDocument, DocGuard, DocGuardMut, Document, DocumentConfig, EventDriver, EventHandler,
    NodeId, UserAgentStylesheet,
};
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzDataTransfer, BlitzDataTransferArc, BlitzDragDataStoreMode, BlitzDragEvent,
    BlitzDragOperation, DomEvent, DomEventData, EventState, PointerCoords, UiEvent,
};
use std::{cell::RefCell, rc::Rc};

/// What a handler saw of an event: its name, its target and the dragged text
type Seen = (&'static str, NodeId, Option<String>);

/// Records drag events, and accepts drops on `accepts` and its descendants
#[derive(Clone, Default)]
struct Handler {
    seen: Rc<RefCell<Vec<Seen>>>,
    accepts: Option<NodeId>,
    drop_effect: Option<&'static str>,
}

impl Handler {
    fn take(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.borrow_mut())
    }
}

impl EventHandler for Handler {
    fn handle_event(
        &mut self,
        chain: &[NodeId],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        let (DomEventData::DragEnter(data)
        | DomEventData::DragOver(data)
        | DomEventData::DragLeave(data)
        | DomEventData::Drop(data)) = &event.data
        else {
            return;
        };
        let text = data.data_transfer().get_data("text/plain");
        self.seen
            .borrow_mut()
            .push((event.name(), event.target, text));

        let accepted = self.accepts.is_some_and(|id| chain.contains(&id));
        if accepted
            && matches!(
                event.data,
                DomEventData::DragOver(_) | DomEventData::Drop(_)
            )
        {
            if let Some(drop_effect) = self.drop_effect {
                data.data_transfer_mut().set_drop_effect_str(drop_effect);
            }
            event_state.prevent_default();
        }
    }
}

fn document(html: &str) -> BaseDocument {
    let config = DocumentConfig {
        ua_stylesheets: vec![UserAgentStylesheet::default_sheet()],
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(html, config).into_inner();
    doc.viewport_mut().window_size = (300, 300);
    doc.resolve(0.0);
    doc
}

fn node(doc: &BaseDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

/// A drag of the text "hello" from another application
fn text_drag() -> BlitzDataTransferArc {
    let mut data_transfer = BlitzDataTransfer::new();
    data_transfer.set_data_unchecked("text/plain", "hello".to_string());
    BlitzDragEvent::new(coords(0.0, 0.0), data_transfer).data_transfer
}

fn coords(x: f32, y: f32) -> PointerCoords {
    PointerCoords {
        page_x: x,
        page_y: y,
        screen_x: x,
        screen_y: y,
        client_x: x,
        client_y: y,
    }
}

fn drag_event(drag: &BlitzDataTransferArc, x: f32, y: f32) -> BlitzDragEvent {
    let mut event = BlitzDragEvent::new(coords(x, y), BlitzDataTransfer::new());
    event.data_transfer = drag.clone();
    event
}

fn send(doc: &mut dyn Document, handler: &Handler, event: UiEvent) {
    EventDriver::new(doc, handler.clone()).handle_ui_event(event);
}

/// Move a drag to `(x, y)`, returning the operation a drop there would perform
fn drag_over(
    doc: &mut dyn Document,
    handler: &Handler,
    drag: &BlitzDataTransferArc,
    (x, y): (f32, f32),
) -> BlitzDragOperation {
    // The shell proposes the operation the drag source prefers (none, if it has no
    // preference) before each move
    drag.borrow_mut().set_drop_effect(BlitzDragOperation::None);
    send(doc, handler, UiEvent::DragOver(drag_event(drag, x, y)));
    drag.borrow().drop_effect()
}

const ZONES: &str = r#"<html><body style="margin:0">
<div id="zone" style="width:100px;height:100px"><div id="child" style="width:50px;height:50px"></div></div>
<div id="other" style="width:100px;height:100px"></div>
</body></html>"#;

#[test]
fn drag_events_follow_the_node_under_the_drag() {
    let mut doc = document(ZONES);
    let [zone, child, other] = ["#zone", "#child", "#other"].map(|s| node(&doc, s));
    let handler = Handler {
        accepts: Some(zone),
        ..Default::default()
    };
    let drag = text_drag();

    // The dragged data can't be read until it is dropped
    let op = drag_over(&mut doc, &handler, &drag, (75.0, 75.0));
    assert_eq!(op, BlitzDragOperation::Copy);
    assert_eq!(
        handler.take(),
        [("dragenter", zone, None), ("dragover", zone, None)]
    );

    // `dragenter` is fired at the new target before `dragleave` at the old one
    let op = drag_over(&mut doc, &handler, &drag, (25.0, 25.0));
    assert_eq!(op, BlitzDragOperation::Copy);
    assert_eq!(
        handler.take(),
        [
            ("dragenter", child, None),
            ("dragleave", zone, None),
            ("dragover", child, None)
        ]
    );

    // Nothing cancels `dragover` here, so a drop is refused...
    let op = drag_over(&mut doc, &handler, &drag, (50.0, 150.0));
    assert_eq!(op, BlitzDragOperation::None);
    handler.take();

    // ...and a refused drop is a drag that left
    send(
        &mut doc,
        &handler,
        UiEvent::Drop(drag_event(&drag, 50.0, 150.0)),
    );
    assert_eq!(handler.take(), [("dragleave", other, None)]);

    // An accepted drop can read the data
    drag_over(&mut doc, &handler, &drag, (75.0, 75.0));
    send(
        &mut doc,
        &handler,
        UiEvent::Drop(drag_event(&drag, 75.0, 75.0)),
    );
    assert_eq!(handler.take()[2], ("drop", zone, Some("hello".to_string())));

    // The drag ended with the drop, so there is nothing left to leave
    send(
        &mut doc,
        &handler,
        UiEvent::DragLeave(drag_event(&drag, 75.0, 75.0)),
    );
    assert_eq!(handler.take(), []);
}

#[test]
fn drag_leaving_the_document_fires_dragleave() {
    let mut doc = document(ZONES);
    let zone = node(&doc, "#zone");
    let handler = Handler::default();
    let drag = text_drag();

    drag_over(&mut doc, &handler, &drag, (75.0, 75.0));
    handler.take();
    send(
        &mut doc,
        &handler,
        UiEvent::DragLeave(drag_event(&drag, 75.0, -1.0)),
    );
    assert_eq!(handler.take(), [("dragleave", zone, None)]);
}

#[test]
fn drop_effect_must_be_allowed_by_the_drag_source() {
    let mut doc = document(ZONES);
    let zone = node(&doc, "#zone");
    let drag = text_drag();
    {
        let mut data_transfer = drag.borrow_mut();
        data_transfer.set_mode(BlitzDragDataStoreMode::ReadWrite);
        data_transfer.set_effect_allowed_str("link");
    }

    // `dropEffect` starts out as an operation the source allows
    let handler = Handler {
        accepts: Some(zone),
        ..Default::default()
    };
    let op = drag_over(&mut doc, &handler, &drag, (75.0, 75.0));
    assert_eq!(op, BlitzDragOperation::Link);

    // A handler asking for one it doesn't allow refuses the drop
    let handler = Handler {
        accepts: Some(zone),
        drop_effect: Some("copy"),
        ..Default::default()
    };
    let op = drag_over(&mut doc, &handler, &drag, (75.0, 75.0));
    assert_eq!(op, BlitzDragOperation::None);
}

/// A sub-document whose events are seen by `handler`
struct Child {
    doc: BaseDocument,
    handler: Handler,
}

impl Document for Child {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::Ref(&self.doc)
    }
    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::Ref(&mut self.doc)
    }
    fn handle_ui_event(&mut self, event: UiEvent) {
        EventDriver::new(&mut self.doc, self.handler.clone()).handle_ui_event(event);
    }
}

#[test]
fn sub_document_accepts_and_leaves_a_drag_over_its_host() {
    let mut parent = document(
        r#"<html><body style="margin:0">
        <div style="height:50px"></div>
        <div id="host" style="width:300px;height:100px"></div>
        <div id="outside" style="width:300px;height:100px"></div>
        </body></html>"#,
    );
    let child = document(
        r#"<html><body style="margin:0"><div id="inner" style="width:200px;height:50px"></div></body></html>"#,
    );
    let host = node(&parent, "#host");
    let inner = node(&child, "#inner");
    let child_handler = Handler {
        accepts: Some(inner),
        ..Default::default()
    };
    let child = Child {
        doc: child,
        handler: child_handler.clone(),
    };
    parent.set_sub_document(host, Box::new(child));
    parent.resolve(0.0);
    let parent_handler = Handler::default();
    let drag = text_drag();

    // The sub-document decides whether a drop on its host is accepted
    let op = drag_over(&mut parent, &parent_handler, &drag, (100.0, 75.0));
    assert_eq!(op, BlitzDragOperation::Copy);
    assert_eq!(
        parent_handler.take(),
        [("dragenter", host, None), ("dragover", host, None)]
    );
    assert_eq!(
        child_handler.take(),
        [("dragenter", inner, None), ("dragover", inner, None)]
    );

    // A drag that leaves the host leaves the sub-document
    let op = drag_over(&mut parent, &parent_handler, &drag, (100.0, 200.0));
    assert_eq!(op, BlitzDragOperation::None);
    assert_eq!(child_handler.take(), [("dragleave", inner, None)]);

    // A drop on the host is a drop in the sub-document
    drag_over(&mut parent, &parent_handler, &drag, (100.0, 75.0));
    child_handler.take();
    send(
        &mut parent,
        &parent_handler,
        UiEvent::Drop(drag_event(&drag, 100.0, 75.0)),
    );
    assert_eq!(
        child_handler.take(),
        [("drop", inner, Some("hello".to_string()))]
    );
}
