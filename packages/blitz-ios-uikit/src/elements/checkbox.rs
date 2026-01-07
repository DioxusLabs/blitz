//! Checkbox element (UISwitch) implementation
//!
//! Maps `<input type="checkbox">` to UISwitch.

use std::cell::Cell;

use blitz_dom::Node;
use markup5ever::local_name;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::{UIControlEvents, UISwitch, UIView};

use crate::events::EventSender;

// =============================================================================
// BlitzSwitch - Custom UISwitch with event bridging
// =============================================================================

/// Ivars for BlitzSwitch
#[derive(Default)]
pub struct BlitzSwitchIvars {
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UISwitch subclass that bridges value change events to blitz-dom.
    #[unsafe(super(UISwitch))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzSwitch"]
    #[ivars = BlitzSwitchIvars]
    pub struct BlitzSwitch;

    unsafe impl NSObjectProtocol for BlitzSwitch {}

    impl BlitzSwitch {
        #[unsafe(method(handleValueChanged:))]
        fn handle_value_changed(&self, sender: &UISwitch) {
            let node_id = self.ivars().node_id.get();
            let is_on = unsafe { sender.isOn() };

            #[cfg(debug_assertions)]
            println!(
                "[BlitzSwitch] value changed for node_id={}, is_on={}",
                node_id, is_on
            );

            // TODO: Send input/change event via EventSender
        }
    }
);

impl BlitzSwitch {
    /// Create a new BlitzSwitch.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzSwitchIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let switch: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Add target-action for value changed
        unsafe {
            switch.addTarget_action_forControlEvents(
                Some(&*switch),
                sel!(handleValueChanged:),
                UIControlEvents::ValueChanged,
            );
        }

        switch
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }
}

/// Create a UISwitch for a checkbox input.
pub fn create_switch(
    mtm: MainThreadMarker,
    node: &Node,
    node_id: usize,
    _event_sender: &EventSender,
) -> Retained<UIView> {
    let switch = BlitzSwitch::new(mtm, node_id);

    // Apply initial state
    apply_switch_state(&switch, node);

    // Cast to UIView
    unsafe { Retained::cast(switch) }
}

/// Update a UISwitch with new node data.
pub fn update_switch(view: &UIView, node: &Node) {
    // SAFETY: We only call this for Switch element types
    let switch: &UISwitch = unsafe { std::mem::transmute(view) };
    apply_switch_state(switch, node);
}

/// Apply state from node to switch.
fn apply_switch_state(switch: &UISwitch, node: &Node) {
    let Some(element_data) = node.element_data() else {
        return;
    };

    // Check if checked
    let checked = element_data.attr(local_name!("checked")).is_some();
    unsafe { switch.setOn_animated(checked, false) };

    // Check if disabled
    let disabled = element_data.attr(local_name!("disabled")).is_some();
    unsafe { switch.setEnabled(!disabled) };
}
