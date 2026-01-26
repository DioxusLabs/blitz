use std::any::Any;

use accesskit::Node as AccessKitNode;
use blitz_traits::events::UiEvent;
use style::properties::ComputedValues;
use taffy::{LayoutInput, LayoutOutput};

pub use anyrender::{RenderContext, Scene};

use crate::BaseDocument;

impl BaseDocument {
    pub fn can_create_surfaces(&mut self, render_context: &dyn RenderContext) {
        for &node_id in self.custom_widget_nodes.iter() {
            let node = &mut self.nodes[node_id];

            // TODO:
            // - Downcast to element
            // - Downcast to custom widget element (add variant to SpecialElementData)
            // - Call can_create_surfaces on custom widget
        }
    }

    pub fn destroy_surfaces(&mut self) {
        for &node_id in self.custom_widget_nodes.iter() {
            let node = &mut self.nodes[node_id];

            // TODO:
            // See above but for destroy_surfaces
        }
    }
}

pub trait Widget {
    // DOM lifecycle

    /// The widget was attached to the DOM
    fn connected(&mut self) {}
    /// The widget was removed from the DOM
    fn disconnected(&mut self) {}
    /// One of the widget's attributes changed
    fn attribute_changed(&mut self, name: &str, old_value: Option<&str>, new_value: Option<&str>) {
        let _ = (name, old_value, new_value);
    }

    // Renderer lifecycle

    /// The renderer is active
    ///
    /// `ctx` parameter can be downcast to get access to renderer-specific contexts (e.g. the WGPU Device and Queue)
    fn can_create_surfaces(&mut self, render_ctx: &dyn RenderContext) {
        let _ = render_ctx;
    }
    /// The renderer is no longer active (destroy textures here)
    fn destroy_surfaces(&mut self) {}

    // Other

    /// Handle input events (mouse, keyboard, etc)
    fn handle_event(&mut self, event: &UiEvent) {
        let _ = event;
    }

    /// Callback for the widget to paint it's content.
    ///
    /// Output is recorded to an AnyRender `Scene`.
    /// If the widget wants to render to a WGPU texture or similar then it should:
    ///   - Get a handle to the Device and Queue in `can_create_surfaces`
    ///   - Create it's own texture
    ///   - Pass the `ResourceId` of the paint for an Image in the AnyRender `Scene`
    fn paint(
        &mut self,
        render_ctx: &dyn RenderContext,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Scene {
        let _ = (render_ctx, width, height, scale);
        Scene::new()
    }

    // TODO: allow for multiple nodes per widget
    // fn accessibility_tree(&mut self) -> AccessKitNode;

    // TODO: simpler layout mode?
    // fn layout(&mut self, inputs: LayoutInput, styles: &ComputedValues) -> LayoutOutput;
}
