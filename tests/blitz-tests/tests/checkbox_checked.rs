//! Regression test for checkbox `checked` attribute reactivity.
//! See https://github.com/DioxusLabs/dioxus/issues/5282
//!
//! When a signal drives the `checked` attribute of an `<input type="checkbox">`,
//! toggling the signal off clears the attribute but must also update the
//! element's checkedness (stored in `SpecialElementData::CheckboxInput`).

use blitz_test_harness::HarnessOptions;
use dioxus::prelude::*;
use dioxus_core::ScopeId;
use dioxus_native_dom::DioxusDocument;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Props, Clone)]
struct AppProps {
    checked: Rc<Cell<bool>>,
}

impl PartialEq for AppProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.checked, &other.checked)
    }
}

fn app(props: AppProps) -> Element {
    let checked = props.checked.get();
    rsx! {
        input {
            id: "cb",
            type: "checkbox",
            checked,
        }
    }
}

struct Harness {
    inner: blitz_test_harness::Harness<DioxusDocument>,
    checked: Rc<Cell<bool>>,
}

impl Harness {
    fn new(initial: bool) -> Self {
        let checked = Rc::new(Cell::new(initial));
        let vdom = VirtualDom::new_with_props(
            app,
            AppProps {
                checked: Rc::clone(&checked),
            },
        );
        let inner = blitz_test_harness::Harness::from_vdom(vdom, HarnessOptions::default());
        Self { inner, checked }
    }

    fn set_checked(&mut self, value: bool) {
        self.checked.set(value);
        self.inner.doc.vdom.mark_dirty(ScopeId::APP);
        self.inner.pump();
    }

    fn checkbox_state(&self) -> bool {
        let node_id = self.inner.node("#cb");
        let doc = self.inner.doc.inner.borrow();
        let node = doc.get_node(node_id).unwrap();
        node.element_data()
            .unwrap()
            .checkbox_input_checked()
            .expect("element is not a checkbox")
    }
}

#[test]
fn checked_attribute_is_reactive() {
    let mut harness = Harness::new(false);
    assert!(
        !harness.checkbox_state(),
        "initial state should be unchecked"
    );

    harness.set_checked(true);
    assert!(
        harness.checkbox_state(),
        "should be checked after set to true"
    );

    harness.set_checked(false);
    assert!(
        !harness.checkbox_state(),
        "should be unchecked after set back to false"
    );

    harness.set_checked(true);
    assert!(
        harness.checkbox_state(),
        "should be checked after set to true again"
    );
}

#[test]
fn checked_attribute_initially_true_is_reactive() {
    let mut harness = Harness::new(true);
    assert!(harness.checkbox_state(), "initial state should be checked");

    harness.set_checked(false);
    assert!(
        !harness.checkbox_state(),
        "should be unchecked after set to false"
    );
}
