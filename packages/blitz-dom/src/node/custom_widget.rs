use std::any::Any;

use anyrender::ResourceId;
use blitz_traits::events::{BlitzPointerId, UiEvent};
use markup5ever::QualName;
pub use style::properties::ComputedValues as ComputedStyles;
// use accesskit::Node as AccessKitNode;
// use taffy::{LayoutInput, LayoutOutput};

pub use anyrender::{RenderContext, Scene};

use crate::BaseDocument;

impl BaseDocument {
    pub fn can_create_surfaces(&mut self, render_context: &mut dyn RenderContext) {
        for &node_id in self.custom_widget_nodes.iter() {
            let node = &mut self.nodes[node_id];
            if let Some(widget_data) = node
                .element_data_mut()
                .and_then(|el| el.custom_widget_data_mut())
            {
                let mut render_context = ProxyRenderContext {
                    resource_ids: &mut widget_data.active_resource_ids,
                    inner: render_context,
                };

                widget_data
                    .widget
                    .can_create_surfaces(&mut render_context as _);
            }
        }
    }

    pub fn destroy_surfaces(&mut self) {
        for &node_id in self.custom_widget_nodes.iter() {
            let node = &mut self.nodes[node_id];
            if let Some(widget_data) = node
                .element_data_mut()
                .and_then(|el| el.custom_widget_data_mut())
            {
                widget_data.widget.destroy_surfaces();
            }
        }
    }
}

/// A `RenderContext` that proxies resource registrations through to an inner `RenderContext`
/// and also keeps track of the `ResourceId`s of all sucessfully registered resources so that
/// they can be automatically unregistered if the Widget's node is dropped.
pub struct ProxyRenderContext<'widget, 'rend> {
    pub resource_ids: &'widget mut Vec<ResourceId>,
    pub inner: &'rend mut dyn RenderContext,
}

impl anyrender::RenderContext for ProxyRenderContext<'_, '_> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<ResourceId, anyrender::RegisterResourceError> {
        let id = self.inner.try_register_custom_resource(resource)?;
        self.resource_ids.push(id);
        Ok(id)
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        self.resource_ids.retain(|id| *id != resource_id);
        self.inner.unregister_resource(resource_id);
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn std::any::Any>> {
        self.inner.renderer_specific_context()
    }
}

/// A pointer capture change queued by a widget while handling an event
pub(crate) enum PointerCaptureOp {
    Set(BlitzPointerId),
    Release(BlitzPointerId),
}

/// Context passed to [`Widget::paint`].
///
/// Allows the widget to feed requests back to the document while painting.
#[derive(Default)]
pub struct WidgetPaintContext {
    redraw_requested: bool,
}

impl WidgetPaintContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Request that the document be redrawn.
    ///
    /// Widgets which animate should call this each time they paint to schedule another frame.
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    /// Whether the widget has requested a redraw
    pub fn redraw_requested(&self) -> bool {
        self.redraw_requested
    }
}

/// Context passed to [`Widget::handle_event`].
///
/// Provides the widget with information about its environment (such as the size of its box),
/// and allows it to feed changes back into the document (attribute updates, DOM input events,
/// redraw requests). Changes are queued and applied by the document once event handling completes.
pub struct WidgetEventContext {
    /// The width of the widget's border box in CSS pixels
    pub width: f32,
    /// The height of the widget's border box in CSS pixels
    pub height: f32,
    /// The scale factor of the document's viewport
    pub scale: f64,
    /// Attributes that the widget wants to set on its element
    pub(crate) queued_attributes: Vec<(QualName, String)>,
    /// Values for DOM "input" events that the widget wants to dispatch from its element
    pub(crate) queued_input_events: Vec<String>,
    /// Pointer capture changes that the widget wants to make
    pub(crate) queued_pointer_capture_ops: Vec<PointerCaptureOp>,
    /// Whether the widget has requested a redraw
    pub(crate) redraw_requested: bool,
}

impl WidgetEventContext {
    pub(crate) fn new(width: f32, height: f32, scale: f64) -> Self {
        Self {
            width,
            height,
            scale,
            queued_attributes: Vec::new(),
            queued_input_events: Vec::new(),
            queued_pointer_capture_ops: Vec::new(),
            redraw_requested: false,
        }
    }

    /// Set an attribute on the widget's element (applied once event handling completes)
    pub fn set_attribute(&mut self, name: QualName, value: String) {
        self.queued_attributes.push((name, value));
    }

    /// Dispatch a DOM "input" event with the given value, targeting the widget's element
    pub fn dispatch_input_event(&mut self, value: String) {
        self.queued_input_events.push(value);
    }

    /// Capture the given pointer so that the widget's element continues to receive pointer
    /// events (e.g. `PointerMove`) for that pointer, even when the pointer is outside of the
    /// element's bounds. The capture is automatically released when the pointer is released.
    pub fn set_pointer_capture(&mut self, pointer_id: BlitzPointerId) {
        self.queued_pointer_capture_ops
            .push(PointerCaptureOp::Set(pointer_id));
    }

    /// Release a pointer capture previously set with
    /// [`set_pointer_capture`](Self::set_pointer_capture)
    pub fn release_pointer_capture(&mut self, pointer_id: BlitzPointerId) {
        self.queued_pointer_capture_ops
            .push(PointerCaptureOp::Release(pointer_id));
    }

    /// Request that the document be redrawn
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
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
    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        let _ = render_ctx;
    }
    /// The renderer is no longer active (destroy textures here)
    fn destroy_surfaces(&mut self) {}

    // Other

    /// Handle input events (mouse, keyboard, etc)
    ///
    /// Pointer event coordinates are relative to the widget's border box.
    /// The `ctx` parameter provides the size of the widget's box and allows the widget
    /// to queue changes (attribute updates, DOM input events, pointer captures, redraw
    /// requests) which are applied by the document once event handling completes.
    ///
    /// By default pointer events are only received while the pointer is within the widget's
    /// bounds. Use [`WidgetEventContext::set_pointer_capture`] to continue receiving
    /// `PointerMove`/`PointerUp` events for a pointer when it moves outside of the widget's
    /// bounds (e.g. for implementing drag interactions).
    fn handle_event(&mut self, event: &UiEvent, ctx: &mut WidgetEventContext) {
        let _ = (event, ctx);
    }

    /// Callback for the widget to paint it's content.
    ///
    /// Output is recorded to an AnyRender `Scene`.
    /// If the widget wants to render to a WGPU texture or similar then it should:
    ///   - Get a handle to the Device and Queue in `can_create_surfaces`
    ///   - Create it's own texture
    ///   - Pass the `ResourceId` of the paint for an Image in the AnyRender `Scene`
    ///
    /// Widgets are only repainted when the document is redrawn. Widgets which animate
    /// should call [`WidgetPaintContext::request_redraw`] each time they paint to
    /// schedule another frame.
    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
        ctx: &mut WidgetPaintContext,
    ) -> Scene {
        let _ = (render_ctx, styles, width, height, scale, ctx);
        Scene::new()
    }

    // TODO: allow for multiple nodes per widget
    // fn accessibility_tree(&mut self) -> AccessKitNode;

    // TODO: simpler layout mode?
    // fn layout(&mut self, inputs: LayoutInput, styles: &ComputedStyles) -> LayoutOutput;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CustomWidgetStatus {
    Suspended,
    Active,
    PendingRemoval,
}

pub struct CustomWidgetData {
    /// The custom widget
    pub widget: Box<dyn Widget>,
    /// The custom widget's status
    pub status: CustomWidgetStatus,
    /// The IDs of active resources
    /// (stored so that we can automatically unregister them if/when the widget is destroyed).
    pub active_resource_ids: Vec<ResourceId>,
}

impl CustomWidgetData {
    pub(crate) fn new(widget: Box<dyn Widget>) -> Self {
        Self {
            widget,
            status: CustomWidgetStatus::Suspended,
            active_resource_ids: Vec::new(),
        }
    }

    pub(crate) fn take_resource_ids(&mut self) -> Vec<ResourceId> {
        core::mem::take(&mut self.active_resource_ids)
    }
}
