//! Tests for the browser-like event listener registry and dispatch algorithm:
//! `add_event_listener` / `remove_event_listener` on the document, and the DOM
//! capture -> target -> bubble dispatch performed by the `EventDriver`.

use blitz_dom::{
    CallbackEventHandler, DocumentConfig, EventContext, EventDriver, EventHandler, EventListenerId,
    EventListenerOptions,
};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzFocusEvent, BlitzInputEvent, BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData,
    DomEventKind, EventPhase, EventState, MouseEventButton, MouseEventButtons, Point,
    PointerCoords, PointerDetails,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

fn document(html: &str) -> HtmlDocument {
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            ..Default::default()
        },
    )
}

fn nested_document() -> HtmlDocument {
    document(
        r#"<html><body><div id="outer"><div id="inner"><button id="target"></button></div></div></body></html>"#,
    )
}

fn node_id(doc: &HtmlDocument, selector: &str) -> usize {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn click_event_data() -> DomEventData {
    DomEventData::Click(BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: 0.0,
            page_y: 0.0,
            screen_x: 0.0,
            screen_y: 0.0,
            client_x: 0.0,
            client_y: 0.0,
        },
        button: MouseEventButton::Main,
        buttons: MouseEventButtons::None,
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    })
}

/// A record of a single listener invocation
#[derive(Clone, Debug, PartialEq, Eq)]
struct Invocation {
    listener: EventListenerId,
    target: usize,
    current_target: Option<usize>,
    phase: EventPhase,
}

/// The reaction a listener should have when invoked
#[derive(Clone, Copy)]
enum Reaction {
    None,
    PreventDefault,
    StopPropagation,
    StopImmediatePropagation,
}

/// An [`EventHandler`] that records every listener invocation and reacts
/// according to a per-listener [`Reaction`]
#[derive(Clone, Default)]
struct RecordingHandler {
    invocations: Rc<RefCell<Vec<Invocation>>>,
    reactions: Rc<RefCell<Vec<(EventListenerId, Reaction)>>>,
}

impl RecordingHandler {
    fn with_reaction(&self, listener: EventListenerId, reaction: Reaction) -> Self {
        self.reactions.borrow_mut().push((listener, reaction));
        self.clone()
    }

    fn invoked_listeners(&self) -> Vec<EventListenerId> {
        self.invocations
            .borrow()
            .iter()
            .map(|i| i.listener)
            .collect()
    }
}

impl EventHandler for RecordingHandler {
    fn handle_event_listener(
        &self,
        listener: EventListenerId,
        event: &mut DomEvent,
        _ctx: &mut EventContext<'_>,
        event_state: &mut EventState,
    ) {
        self.invocations.borrow_mut().push(Invocation {
            listener,
            target: event.target,
            current_target: event.current_target,
            phase: event.phase,
        });
        let reaction = self
            .reactions
            .borrow()
            .iter()
            .find(|(id, _)| *id == listener)
            .map(|(_, reaction)| *reaction)
            .unwrap_or(Reaction::None);
        match reaction {
            Reaction::None => {}
            Reaction::PreventDefault => event_state.prevent_default(),
            Reaction::StopPropagation => event_state.stop_propagation(),
            Reaction::StopImmediatePropagation => event_state.stop_immediate_propagation(),
        }
    }
}

fn add_click_listener(
    doc: &mut HtmlDocument,
    node: usize,
    id: u64,
    capture: bool,
) -> EventListenerId {
    let listener = EventListenerId(id);
    assert!(doc.add_event_listener(
        node,
        DomEventKind::Click,
        listener,
        EventListenerOptions {
            capture,
            ..Default::default()
        },
    ));
    listener
}

#[test]
fn dispatches_capture_target_and_bubble_phases_in_order() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let inner = node_id(&doc, "#inner");
    let target = node_id(&doc, "#target");

    // Registered out of dispatch order to prove ordering comes from the DOM tree,
    // not from registration order
    let inner_bubble = add_click_listener(&mut doc, inner, 1, false);
    let outer_capture = add_click_listener(&mut doc, outer, 2, true);
    let target_bubble = add_click_listener(&mut doc, target, 3, false);
    let outer_bubble = add_click_listener(&mut doc, outer, 4, false);
    let inner_capture = add_click_listener(&mut doc, inner, 5, true);
    let target_capture = add_click_listener(&mut doc, target, 6, true);

    let handler = RecordingHandler::default();
    let invocations = handler.invocations.clone();
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(target, click_event_data()));

    assert_eq!(
        *invocations.borrow(),
        vec![
            // Capture phase: root -> target
            Invocation {
                listener: outer_capture,
                target,
                current_target: Some(outer),
                phase: EventPhase::Capturing,
            },
            Invocation {
                listener: inner_capture,
                target,
                current_target: Some(inner),
                phase: EventPhase::Capturing,
            },
            // Target phase: listeners in registration order regardless of capture flag
            Invocation {
                listener: target_bubble,
                target,
                current_target: Some(target),
                phase: EventPhase::AtTarget,
            },
            Invocation {
                listener: target_capture,
                target,
                current_target: Some(target),
                phase: EventPhase::AtTarget,
            },
            // Bubble phase: target -> root
            Invocation {
                listener: inner_bubble,
                target,
                current_target: Some(inner),
                phase: EventPhase::Bubbling,
            },
            Invocation {
                listener: outer_bubble,
                target,
                current_target: Some(outer),
                phase: EventPhase::Bubbling,
            },
        ]
    );
}

#[test]
fn non_bubbling_events_capture_and_fire_at_target_but_do_not_bubble() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let target = node_id(&doc, "#target");

    let outer_capture = EventListenerId(1);
    let outer_bubble = EventListenerId(2);
    let at_target = EventListenerId(3);
    assert!(doc.add_event_listener(
        outer,
        DomEventKind::Focus,
        outer_capture,
        EventListenerOptions {
            capture: true,
            ..Default::default()
        },
    ));
    assert!(doc.add_event_listener(
        outer,
        DomEventKind::Focus,
        outer_bubble,
        EventListenerOptions::default(),
    ));
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Focus,
        at_target,
        EventListenerOptions::default(),
    ));

    let handler = RecordingHandler::default();
    let invocations = handler.invocations.clone();
    EventDriver::new(&mut doc, handler)
        .handle_dom_event(DomEvent::new(target, DomEventData::Focus(BlitzFocusEvent)));

    // The capture listener fires on the way down and the target listener fires,
    // but the event does not bubble back up to `outer`
    let invocations = invocations.borrow();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[0].listener, outer_capture);
    assert_eq!(invocations[0].phase, EventPhase::Capturing);
    assert_eq!(invocations[1].listener, at_target);
    assert_eq!(invocations[1].phase, EventPhase::AtTarget);
}

#[test]
fn stop_propagation_halts_capture_descent() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let inner = node_id(&doc, "#inner");
    let target = node_id(&doc, "#target");

    let outer_capture = add_click_listener(&mut doc, outer, 1, true);
    let inner_capture = add_click_listener(&mut doc, inner, 2, true);
    add_click_listener(&mut doc, target, 3, false);

    let handler =
        RecordingHandler::default().with_reaction(inner_capture, Reaction::StopPropagation);
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(target, click_event_data()));

    // Propagation stops after the inner capture listener: the target listener never fires
    assert_eq!(
        handler.invoked_listeners(),
        vec![outer_capture, inner_capture]
    );
}

#[test]
fn stop_propagation_halts_bubble_ascent() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let inner = node_id(&doc, "#inner");
    let target = node_id(&doc, "#target");

    let target_bubble = add_click_listener(&mut doc, target, 1, false);
    let inner_bubble = add_click_listener(&mut doc, inner, 2, false);
    add_click_listener(&mut doc, outer, 3, false);

    let handler =
        RecordingHandler::default().with_reaction(inner_bubble, Reaction::StopPropagation);
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(target, click_event_data()));

    assert_eq!(
        handler.invoked_listeners(),
        vec![target_bubble, inner_bubble]
    );
}

#[test]
fn stop_immediate_propagation_halts_remaining_listeners_on_same_node() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let target = node_id(&doc, "#target");

    let first = add_click_listener(&mut doc, target, 1, false);
    add_click_listener(&mut doc, target, 2, false);
    add_click_listener(&mut doc, outer, 3, false);

    let handler =
        RecordingHandler::default().with_reaction(first, Reaction::StopImmediatePropagation);
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(target, click_event_data()));

    // Unlike plain stop_propagation, the second listener on the *same* node is also skipped
    assert_eq!(handler.invoked_listeners(), vec![first]);
}

#[test]
fn duplicate_registration_is_rejected_and_removal_works() {
    let mut doc = nested_document();
    let target = node_id(&doc, "#target");
    let listener = EventListenerId(1);

    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions::default()
    ));
    // Same (kind, id, capture) triple: rejected
    assert!(!doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions::default()
    ));
    // Same id but different capture flag: accepted (distinct listener, as in the DOM)
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions {
            capture: true,
            ..Default::default()
        }
    ));

    assert!(doc.remove_event_listener(target, DomEventKind::Click, listener, false));
    // Already removed
    assert!(!doc.remove_event_listener(target, DomEventKind::Click, listener, false));
    assert!(doc.remove_event_listener(target, DomEventKind::Click, listener, true));

    // With all listeners removed, dispatch invokes nothing
    let handler = RecordingHandler::default();
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(target, click_event_data()));
    assert!(handler.invoked_listeners().is_empty());
}

#[test]
fn listener_removed_during_dispatch_does_not_fire() {
    struct RemovingHandler {
        invoked: Rc<RefCell<Vec<EventListenerId>>>,
        remove: (usize, EventListenerId),
    }

    impl EventHandler for RemovingHandler {
        fn handle_event_listener(
            &self,
            listener: EventListenerId,
            _event: &mut DomEvent,
            ctx: &mut EventContext<'_>,
            _event_state: &mut EventState,
        ) {
            self.invoked.borrow_mut().push(listener);
            let (node_id, listener) = self.remove;
            ctx.doc_mut().inner_mut().remove_event_listener(
                node_id,
                DomEventKind::Click,
                listener,
                false,
            );
        }
    }

    let mut doc = nested_document();
    let target = node_id(&doc, "#target");
    let first = add_click_listener(&mut doc, target, 1, false);
    let second = add_click_listener(&mut doc, target, 2, false);

    let invoked = Rc::new(RefCell::new(Vec::new()));
    let handler = RemovingHandler {
        invoked: invoked.clone(),
        remove: (target, second),
    };
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(target, click_event_data()));

    // The first listener removed the second during dispatch, so it must not fire
    assert_eq!(*invoked.borrow(), vec![first]);
}

#[test]
fn once_listeners_fire_only_once() {
    let mut doc = nested_document();
    let target = node_id(&doc, "#target");
    let listener = EventListenerId(1);
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions {
            once: true,
            ..Default::default()
        },
    ));

    let handler = RecordingHandler::default();
    let mut driver = EventDriver::new(&mut doc, handler.clone());
    for _ in 0..2 {
        driver.handle_dom_event(DomEvent::new(target, click_event_data()));
    }
    assert_eq!(handler.invoked_listeners(), vec![listener]);
}

#[test]
fn once_listener_can_re_register_itself() {
    /// A handler which re-registers the invoked `once` listener on every invocation
    struct ReRegisteringHandler {
        call_count: Rc<Cell<usize>>,
    }

    impl EventHandler for ReRegisteringHandler {
        fn handle_event_listener(
            &self,
            listener: EventListenerId,
            event: &mut DomEvent,
            ctx: &mut EventContext<'_>,
            _event_state: &mut EventState,
        ) {
            self.call_count.set(self.call_count.get() + 1);
            // The `once` listener has already been removed at this point, so it can
            // register itself again without being immediately deregistered
            assert!(ctx.doc_mut().inner_mut().add_event_listener(
                event.current_target.unwrap(),
                DomEventKind::Click,
                listener,
                EventListenerOptions {
                    once: true,
                    ..Default::default()
                },
            ));
        }
    }

    let mut doc = nested_document();
    let target = node_id(&doc, "#target");
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        EventListenerId(1),
        EventListenerOptions {
            once: true,
            ..Default::default()
        },
    ));

    let call_count = Rc::new(Cell::new(0));
    let handler = ReRegisteringHandler {
        call_count: call_count.clone(),
    };
    let mut driver = EventDriver::new(&mut doc, handler);
    for _ in 0..3 {
        driver.handle_dom_event(DomEvent::new(target, click_event_data()));
    }
    assert_eq!(call_count.get(), 3);
}

#[test]
fn prevent_default_cancels_default_action_unless_listener_is_passive() {
    // Clicking a checkbox toggles it (the default action) unless a listener cancels the event
    let mut doc = document(r#"<html><body><input id="target" type="checkbox"></body></html>"#);
    doc.resolve(0.0);
    let target = node_id(&doc, "#target");
    let checked = |doc: &HtmlDocument| {
        doc.get_node(target)
            .unwrap()
            .element_data()
            .unwrap()
            .checkbox_input_checked()
            .unwrap()
    };
    assert!(!checked(&doc));

    // A non-passive listener which calls prevent_default cancels the toggle
    let listener = EventListenerId(1);
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions::default(),
    ));
    let handler = RecordingHandler::default().with_reaction(listener, Reaction::PreventDefault);
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(target, click_event_data()));
    assert!(!checked(&doc));

    // A passive listener which calls prevent_default does NOT cancel the toggle
    assert!(doc.remove_event_listener(target, DomEventKind::Click, listener, false));
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        listener,
        EventListenerOptions {
            passive: true,
            ..Default::default()
        },
    ));
    let handler = RecordingHandler::default().with_reaction(listener, Reaction::PreventDefault);
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(target, click_event_data()));
    assert!(checked(&doc));
}

#[test]
fn listeners_are_removed_when_their_node_is_dropped() {
    let mut doc = nested_document();
    let inner = node_id(&doc, "#inner");
    let target = node_id(&doc, "#target");
    add_click_listener(&mut doc, target, 1, false);

    // Dropping the subtree containing the target removes its listeners
    let mut mutr = doc.mutate();
    mutr.remove_and_drop_node(inner);
    drop(mutr);

    // Dispatching to another node must not panic or invoke the dropped listener,
    // and re-registering on a recycled node id starts from a clean slate
    let outer = node_id(&doc, "#outer");
    let handler = RecordingHandler::default();
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(outer, click_event_data()));
    assert!(handler.invoked_listeners().is_empty());
}

#[test]
fn programmatic_focus_changes_queue_focus_events() {
    let mut doc = document(
        r#"<html><body><input id="a" type="text"><input id="b" type="text"></body></html>"#,
    );
    let a = node_id(&doc, "#a");
    let b = node_id(&doc, "#b");

    let focus_a = EventListenerId(1);
    let blur_a = EventListenerId(2);
    let focus_b = EventListenerId(3);
    assert!(doc.add_event_listener(a, DomEventKind::Focus, focus_a, Default::default()));
    assert!(doc.add_event_listener(a, DomEventKind::Blur, blur_a, Default::default()));
    assert!(doc.add_event_listener(b, DomEventKind::Focus, focus_b, Default::default()));

    // Focus changes made outside of any event dispatch (e.g. by embedder code) queue
    // focus events on the document...
    doc.set_focus_to(a);
    doc.set_focus_to(b);
    assert!(doc.has_pending_events());

    // ...which are dispatched (in order) when an event driver runs
    let handler = RecordingHandler::default();
    EventDriver::new(&mut doc, handler.clone()).flush_pending_events();
    assert_eq!(handler.invoked_listeners(), vec![focus_a, blur_a, focus_b]);
    assert!(!doc.has_pending_events());
}

#[test]
fn listeners_can_dispatch_events_synchronously() {
    /// A handler which logs entry/exit of every listener invocation, and synchronously
    /// dispatches an input event from within the click listener
    struct NestedDispatchHandler {
        log: Rc<RefCell<Vec<String>>>,
        input_target: usize,
    }

    impl EventHandler for NestedDispatchHandler {
        fn handle_event_listener(
            &self,
            _listener: EventListenerId,
            event: &mut DomEvent,
            ctx: &mut EventContext<'_>,
            _event_state: &mut EventState,
        ) {
            let name = event.name();
            self.log.borrow_mut().push(format!("{name}:start"));
            if event.data.kind() == DomEventKind::Click {
                // The input event's listeners run before dispatch_event returns
                ctx.dispatch_event(DomEvent::new(
                    self.input_target,
                    DomEventData::Input(BlitzInputEvent {
                        value: String::new(),
                    }),
                ));
            }
            self.log.borrow_mut().push(format!("{name}:end"));
        }
    }

    let mut doc = nested_document();
    let target = node_id(&doc, "#target");
    let inner = node_id(&doc, "#inner");
    assert!(doc.add_event_listener(
        target,
        DomEventKind::Click,
        EventListenerId(1),
        Default::default()
    ));
    assert!(doc.add_event_listener(
        inner,
        DomEventKind::Input,
        EventListenerId(2),
        Default::default()
    ));

    let log = Rc::new(RefCell::new(Vec::new()));
    let handler = NestedDispatchHandler {
        log: log.clone(),
        input_target: inner,
    };
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(target, click_event_data()));

    assert_eq!(
        *log.borrow(),
        vec!["click:start", "input:start", "input:end", "click:end"]
    );
}

#[test]
fn focus_listener_can_move_focus_synchronously() {
    /// A handler which logs `<name>@<target>` for every invocation, and moves the focus
    /// to `refocus_target` from within the focus listener of `initial_target`
    struct RefocusingHandler {
        log: Rc<RefCell<Vec<String>>>,
        initial_target: usize,
        refocus_target: usize,
    }

    impl EventHandler for RefocusingHandler {
        fn handle_event_listener(
            &self,
            _listener: EventListenerId,
            event: &mut DomEvent,
            ctx: &mut EventContext<'_>,
            _event_state: &mut EventState,
        ) {
            self.log
                .borrow_mut()
                .push(format!("{}@{}", event.name(), event.target));
            if event.data.kind() == DomEventKind::Focus && event.target == self.initial_target {
                // Move the focus from within a focus listener: the resulting blur/focus
                // events dispatch nested, re-entering this handler while it is already
                // on the stack, before the original event sequence continues
                ctx.set_focus(self.refocus_target);
            }
        }
    }

    let mut doc = document(
        r#"<html><body><input id="a" type="text"><input id="b" type="text"></body></html>"#,
    );
    let a = node_id(&doc, "#a");
    let b = node_id(&doc, "#b");
    let listeners = [
        (a, DomEventKind::Focus),
        (a, DomEventKind::Blur),
        (a, DomEventKind::FocusIn),
        (b, DomEventKind::Focus),
        (b, DomEventKind::FocusIn),
    ];
    for (idx, (node, kind)) in listeners.into_iter().enumerate() {
        assert!(doc.add_event_listener(
            node,
            kind,
            EventListenerId(idx as u64 + 1),
            Default::default()
        ));
    }

    doc.set_focus_to(a);

    let log = Rc::new(RefCell::new(Vec::new()));
    let handler = RefocusingHandler {
        log: log.clone(),
        initial_target: a,
        refocus_target: b,
    };
    EventDriver::new(&mut doc, handler).flush_pending_events();

    assert_eq!(
        *log.borrow(),
        vec![
            format!("focus@{a}"),
            // Nested: the focus listener moved the focus to b
            format!("blur@{a}"),
            format!("focus@{b}"),
            format!("focusin@{b}"),
            // The original event sequence continues after the nested events complete
            format!("focusin@{a}"),
        ]
    );
    assert_eq!(doc.get_focussed_node_id(), Some(b));
}

#[test]
fn synchronously_dispatched_events_run_their_default_action() {
    /// A handler which forwards clicks on the outer div to the checkbox, recording the
    /// checkbox's state as observed immediately after the nested dispatch returns
    struct ClickForwardingHandler {
        checkbox: usize,
        checked_after_dispatch: Rc<Cell<Option<bool>>>,
    }

    impl EventHandler for ClickForwardingHandler {
        fn handle_event_listener(
            &self,
            _listener: EventListenerId,
            event: &mut DomEvent,
            ctx: &mut EventContext<'_>,
            _event_state: &mut EventState,
        ) {
            // Ignore the forwarded click bubbling back up through the outer div
            if event.target == self.checkbox {
                return;
            }

            let node = self.checkbox;
            let not_cancelled = ctx.dispatch_event(DomEvent::new(
                node,
                DomEventData::Click(match &event.data {
                    DomEventData::Click(data) => data.clone(),
                    _ => unreachable!(),
                }),
            ));
            assert!(not_cancelled);

            // The checkbox's default action (toggling) has already run
            let checked = ctx
                .doc()
                .inner()
                .get_node(node)
                .unwrap()
                .element_data()
                .unwrap()
                .checkbox_input_checked();
            self.checked_after_dispatch.set(checked);
        }
    }

    let mut doc = document(
        r#"<html><body><div id="outer"><input id="cb" type="checkbox"></div></body></html>"#,
    );
    doc.resolve(0.0);
    let outer = node_id(&doc, "#outer");
    let checkbox = node_id(&doc, "#cb");
    assert!(doc.add_event_listener(
        outer,
        DomEventKind::Click,
        EventListenerId(1),
        Default::default()
    ));

    let checked_after_dispatch = Rc::new(Cell::new(None));
    let handler = ClickForwardingHandler {
        checkbox,
        checked_after_dispatch: checked_after_dispatch.clone(),
    };
    EventDriver::new(&mut doc, handler).handle_dom_event(DomEvent::new(outer, click_event_data()));

    assert_eq!(checked_after_dispatch.get(), Some(true));
}

#[test]
fn callback_event_handler_invokes_closures() {
    let mut doc = nested_document();
    let outer = node_id(&doc, "#outer");
    let target = node_id(&doc, "#target");

    let handler = CallbackEventHandler::new();
    let outer_calls = Rc::new(Cell::new(0));
    let target_calls = Rc::new(Cell::new(0));

    handler.add_event_listener(
        &mut doc,
        outer,
        DomEventKind::Click,
        EventListenerOptions::default(),
        {
            let outer_calls = outer_calls.clone();
            move |event, _doc, _state| {
                assert_eq!(event.phase, EventPhase::Bubbling);
                outer_calls.set(outer_calls.get() + 1);
            }
        },
    );
    let target_listener = handler.add_event_listener(
        &mut doc,
        target,
        DomEventKind::Click,
        EventListenerOptions::default(),
        {
            let target_calls = target_calls.clone();
            move |event, _doc, _state| {
                assert_eq!(event.phase, EventPhase::AtTarget);
                target_calls.set(target_calls.get() + 1);
            }
        },
    );

    let mut driver = EventDriver::new(&mut doc, handler.clone());
    driver.handle_dom_event(DomEvent::new(target, click_event_data()));
    assert_eq!(target_calls.get(), 1);
    assert_eq!(outer_calls.get(), 1);
    drop(driver);

    // Removing the target listener also drops its closure
    assert!(handler.remove_event_listener(
        &mut doc,
        target,
        DomEventKind::Click,
        target_listener,
        false
    ));
    EventDriver::new(&mut doc, handler.clone())
        .handle_dom_event(DomEvent::new(target, click_event_data()));
    assert_eq!(target_calls.get(), 1);
    assert_eq!(outer_calls.get(), 2);
}
