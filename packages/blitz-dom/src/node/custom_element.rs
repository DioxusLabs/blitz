//! Custom element support.
//!
//! A *custom element* is a Rust object (`Box<dyn CustomElement>`) that is
//! attached to a host element and controls an attached shadow DOM. This is the
//! native-Rust analogue of a JavaScript custom element defined via
//! `customElements.define`.
//!
//! Custom elements are either:
//!   - Registered against a tag name in the [`CustomElementRegistry`], in which
//!     case any element with that tag name is automatically "upgraded" (a
//!     controller is instantiated and a shadow root attached) when it is
//!     inserted into the document, or
//!   - Attached manually to a specific node via
//!     [`DocumentMutator::set_custom_element`](crate::DocumentMutator::set_custom_element).

use std::any::Any;
use std::collections::HashMap;

use markup5ever::LocalName;

use crate::DocumentMutator;
use crate::node::ShadowRootMode;

/// A factory function that produces a fresh custom element controller.
pub type CustomElementFactory = Box<dyn Fn() -> Box<dyn CustomElement>>;

/// Definition of a registered custom element.
pub struct CustomElementDefinition {
    /// Factory used to instantiate a controller for each matching element.
    pub factory: CustomElementFactory,
    /// The encapsulation mode of the shadow root attached on upgrade.
    pub mode: ShadowRootMode,
    /// Attribute names the controller wishes to observe. `attribute_changed`
    /// is only invoked for attributes in this list (matching the
    /// `observedAttributes` mechanism of the web platform). An empty list means
    /// observe all attributes.
    pub observed_attributes: Vec<LocalName>,
}

impl CustomElementDefinition {
    pub fn new(factory: impl Fn() -> Box<dyn CustomElement> + 'static) -> Self {
        Self {
            factory: Box::new(factory),
            mode: ShadowRootMode::Open,
            observed_attributes: Vec::new(),
        }
    }

    pub fn with_mode(mut self, mode: ShadowRootMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn observing(mut self, attributes: impl IntoIterator<Item = LocalName>) -> Self {
        self.observed_attributes = attributes.into_iter().collect();
        self
    }

    pub fn observes(&self, name: &LocalName) -> bool {
        self.observed_attributes.is_empty() || self.observed_attributes.contains(name)
    }
}

/// A registry mapping custom element tag names to their definitions.
///
/// Analogous to the web platform's `CustomElementRegistry`
/// (`window.customElements`).
#[derive(Default)]
pub struct CustomElementRegistry {
    definitions: HashMap<LocalName, CustomElementDefinition>,
}

impl CustomElementRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a definition against a tag name. Equivalent to
    /// `customElements.define(name, ...)`.
    pub fn define(&mut self, name: LocalName, definition: CustomElementDefinition) {
        self.definitions.insert(name, definition);
    }

    pub fn get(&self, name: &LocalName) -> Option<&CustomElementDefinition> {
        self.definitions.get(name)
    }

    pub fn contains(&self, name: &LocalName) -> bool {
        self.definitions.contains_key(name)
    }
}

/// The data stored on an element node that has been associated with a custom
/// element controller.
pub struct CustomElementData {
    /// The controller. Stored in an `Option` so it can be temporarily taken out
    /// while a lifecycle callback runs (which needs mutable access to the
    /// document the controller is stored in).
    pub(crate) controller: Option<Box<dyn CustomElement>>,
    /// Whether the element has been upgraded (its `connected` callback run).
    pub upgraded: bool,
}

impl CustomElementData {
    pub(crate) fn new(controller: Box<dyn CustomElement>) -> Self {
        Self {
            controller: Some(controller),
            upgraded: false,
        }
    }
}

/// Context handed to [`CustomElement`] lifecycle callbacks, providing scoped
/// access to mutate the element's shadow DOM.
pub struct CustomElementCtx<'a, 'doc> {
    pub(crate) mutator: &'a mut DocumentMutator<'doc>,
    pub(crate) host_id: usize,
    pub(crate) shadow_root_id: usize,
}

impl<'a, 'doc> CustomElementCtx<'a, 'doc> {
    /// The node id of the host element this custom element is attached to.
    pub fn host_id(&self) -> usize {
        self.host_id
    }

    /// The node id of the shadow root controlled by this custom element.
    pub fn shadow_root_id(&self) -> usize {
        self.shadow_root_id
    }

    /// Escape hatch giving direct access to the [`DocumentMutator`].
    ///
    /// New nodes created via the mutator should be appended into the shadow
    /// tree (see [`shadow_root_id`](Self::shadow_root_id)) for them to be
    /// rendered.
    pub fn mutator(&mut self) -> &mut DocumentMutator<'doc> {
        self.mutator
    }

    /// Read an attribute of the host element.
    pub fn host_attr(&self, name: impl PartialEq<LocalName>) -> Option<String> {
        self.mutator
            .doc
            .get_node(self.host_id)
            .and_then(|node| node.element_data())
            .and_then(|el| el.attr(name))
            .map(|s| s.to_string())
    }

    /// Remove all current children of the shadow root.
    pub fn clear_shadow(&mut self) {
        self.mutator
            .remove_and_drop_all_children(self.shadow_root_id);
    }

    /// Replace the shadow tree contents by parsing the given HTML fragment.
    pub fn set_shadow_html(&mut self, html: &str) {
        self.clear_shadow();
        self.mutator.set_inner_html(self.shadow_root_id, html);
    }

    /// Append already-created nodes as children of the shadow root.
    pub fn append_shadow_children(&mut self, child_ids: &[usize]) {
        self.mutator.append_children(self.shadow_root_id, child_ids);
    }
}

/// A custom element controller: a Rust object that controls the shadow DOM of a
/// host element.
///
/// All callbacks are given a [`CustomElementCtx`] which provides scoped access
/// to mutate the element's shadow tree.
pub trait CustomElement: Any {
    /// Invoked when the element is upgraded / inserted into the document. The
    /// controller should populate its shadow tree here.
    fn connected(&mut self, ctx: &mut CustomElementCtx<'_, '_>) {
        let _ = ctx;
    }

    /// Invoked when the element is removed from the document.
    fn disconnected(&mut self, ctx: &mut CustomElementCtx<'_, '_>) {
        let _ = ctx;
    }

    /// Invoked when an observed attribute of the host element changes.
    fn attribute_changed(
        &mut self,
        ctx: &mut CustomElementCtx<'_, '_>,
        name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
    ) {
        let _ = (ctx, name, old_value, new_value);
    }
}
