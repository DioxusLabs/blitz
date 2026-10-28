use std::any::Any;

use anyrender::ResourceId;
use blitz_traits::events::UiEvent;
pub use style::properties::ComputedValues as ComputedStyles;
// use accesskit::Node as AccessKitNode;
// use taffy::{LayoutInput, LayoutOutput};

pub use anyrender::{RenderContext, Scene};

use crate::BaseDocument;
use crate::layout::replaced::IntrinsicSizes;

/// Requests made by a widget during a callback.
pub struct WidgetCx {
    redraw: bool,
    relayout: bool,
}

impl WidgetCx {
    pub(crate) fn new() -> Self {
        Self {
            redraw: false,
            relayout: false,
        }
    }

    /// Request that the widget be repainted.
    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }

    /// Request layout using the widget's updated intrinsic sizes.
    pub fn request_relayout(&mut self) {
        self.relayout = true;
    }
}

impl BaseDocument {
    pub(crate) fn apply_widget_requests(&mut self, node_id: crate::NodeId, cx: WidgetCx) {
        if cx.relayout {
            let node = &mut self.nodes[node_id];
            node.clear_layout_cache();
            node.insert_damage(crate::layout::damage::ALL_DAMAGE);
        }
        if cx.redraw || cx.relayout {
            self.shell_provider.request_redraw();
        }
    }

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

pub trait Widget {
    // DOM lifecycle

    /// The widget was attached to the DOM
    fn connected(&mut self) {}
    /// The widget was removed from the DOM
    fn disconnected(&mut self) {}
    /// One of the widget's attributes changed
    ///
    /// Existing attributes are replayed on attachment with no old value.
    fn attribute_changed(
        &mut self,
        name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        cx: &mut WidgetCx,
    ) {
        let _ = (name, old_value, new_value, cx);
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

    /// Whether the widget currently requires redraws (e.g. because it is animating).
    ///
    /// Returning `true` causes the document to continuously schedule redraws
    /// (and hence repaints of the widget). Static widgets should return `false`.
    fn requires_redraw(&self) -> bool {
        false
    }

    /// Handle input events (mouse, keyboard, etc)
    fn handle_event(&mut self, event: &UiEvent, cx: &mut WidgetCx) {
        let _ = (event, cx);
    }

    /// The widget's intrinsic dimensions: an intrinsic width, height and
    /// aspect ratio, each of which may independently be absent.
    ///
    /// An absent dimension is sized as it would be without the widget (for a
    /// replaced element, the element's own intrinsic dimension or the default
    /// object size).
    fn intrinsic_sizes(&self) -> IntrinsicSizes {
        IntrinsicSizes::default()
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
        render_ctx: &mut dyn RenderContext,
        styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Scene {
        let _ = (render_ctx, styles, width, height, scale);
        Scene::new()
    }

    // TODO: allow for multiple nodes per widget
    // fn accessibility_tree(&mut self) -> AccessKitNode;

    // TODO: simpler layout mode?
    // fn layout(&mut self, inputs: LayoutInput, styles: &ComputedStyles) -> LayoutOutput;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, DocumentConfig, qual_name};
    use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct CountingShell(AtomicUsize);

    impl ShellProvider for CountingShell {
        fn request_redraw(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct ResizingWidget(f32);

    impl Widget for ResizingWidget {
        fn attribute_changed(
            &mut self,
            name: &str,
            _old: Option<&str>,
            new: Option<&str>,
            cx: &mut WidgetCx,
        ) {
            match name {
                "title" => {
                    self.0 = new.and_then(|value| value.parse().ok()).unwrap_or(32.0);
                    cx.request_relayout();
                }
                "lang" => cx.request_redraw(),
                _ => {}
            }
        }

        fn intrinsic_sizes(&self) -> IntrinsicSizes {
            IntrinsicSizes {
                width: Some(self.0),
                height: Some(self.0 / 2.0),
                ratio: Some(2.0),
            }
        }
    }

    #[test]
    fn widget_cx_requests_relayout_and_redraw() {
        let shell = Arc::new(CountingShell::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            shell_provider: Some(shell.clone()),
            ..Default::default()
        });
        let root = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let parent = mutator.create_element(
            qual_name!("div"),
            vec![Attribute {
                name: qual_name!("style"),
                value: "display:flex; flex-direction:column; align-items:start".into(),
            }],
        );
        let widget = mutator.create_element(
            qual_name!("video"),
            vec![Attribute {
                name: qual_name!("title"),
                value: "64".into(),
            }],
        );
        mutator.append_children(parent, &[widget]);
        mutator.append_children(body, &[parent]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root, &[html]);
        drop(mutator);

        let before = shell.0.load(Ordering::SeqCst);
        doc.set_custom_widget(widget, Box::new(ResizingWidget(0.0)));
        assert_eq!(shell.0.load(Ordering::SeqCst), before + 1);
        doc.resolve(0.0);
        let size = doc.get_node(widget).unwrap().final_layout().size;
        assert_eq!((size.width, size.height), (64.0, 32.0));
        assert_eq!(
            doc.get_node(parent).unwrap().final_layout().size.height,
            32.0
        );

        let before = shell.0.load(Ordering::SeqCst);
        let mut mutator = doc.mutate();
        mutator.set_attribute(widget, qual_name!("title"), "120");
        assert_eq!(shell.0.load(Ordering::SeqCst), before + 1);
        drop(mutator);
        doc.resolve(0.0);
        let size = doc.get_node(widget).unwrap().final_layout().size;
        assert_eq!((size.width, size.height), (120.0, 60.0));
        assert_eq!(
            doc.get_node(parent).unwrap().final_layout().size.height,
            60.0
        );

        doc.mutate().clear_attribute(widget, qual_name!("title"));
        doc.resolve(0.0);
        let size = doc.get_node(widget).unwrap().final_layout().size;
        assert_eq!((size.width, size.height), (32.0, 16.0));
        assert_eq!(
            doc.get_node(parent).unwrap().final_layout().size.height,
            16.0
        );

        let before = shell.0.load(Ordering::SeqCst);
        let mut mutator = doc.mutate();
        mutator.set_attribute(widget, qual_name!("lang"), "en");
        assert_eq!(shell.0.load(Ordering::SeqCst), before + 1);
        mutator.clear_attribute(widget, qual_name!("lang"));
        assert_eq!(shell.0.load(Ordering::SeqCst), before + 2);
    }
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
