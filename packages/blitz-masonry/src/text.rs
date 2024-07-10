use crate::Context;
use accesskit::Role;
use masonry::kurbo::{Point, Size};
use masonry::vello::Scene;
use masonry::widget::{Label, WidgetRef};
use masonry::{
    AccessCtx, AccessEvent, BoxConstraints, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    PointerEvent, StatusChange, TextEvent, Widget, WidgetPod,
};
use smallvec::{smallvec, SmallVec};
use std::sync::Arc;
use taffy::{Dimension, NodeId, Style};

pub struct Text {
    label: WidgetPod<Label>,
    layout_id: Option<NodeId>,
}

impl Text {
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            label: WidgetPod::new(Label::new(content)),
            layout_id: None,
        }
    }
}

impl Widget for Text {
    fn on_pointer_event(&mut self, _ctx: &mut EventCtx, event: &PointerEvent) {}

    fn on_text_event(&mut self, _ctx: &mut EventCtx, _event: &TextEvent) {}

    fn on_access_event(&mut self, _ctx: &mut EventCtx, _event: &AccessEvent) {}

    fn on_status_change(&mut self, _ctx: &mut LifeCycleCtx, event: &StatusChange) {}

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {
        if let LifeCycle::WidgetAdded = event {
            let cx = Context::current().unwrap_or_default();
            ctx.get_mut(&mut self.label)
                .set_text_size(cx.inner.borrow().font_size);
        }

        self.label.lifecycle(ctx, event);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        let size = self.label.layout(ctx, bc);
        ctx.place_child(&mut self.label, Point::ZERO);

        let cx = Context::current().unwrap_or_default();
        let mut cx_ref = cx.inner.borrow_mut();

        let style = Style {
            size: taffy::Size {
                width: Dimension::Length(size.width as _),
                height: Dimension::Length(size.height as _),
            },
            ..Default::default()
        };

        if let Some(layout_id) = self.layout_id {
            cx_ref.taffy.set_style(layout_id, style).unwrap();
        } else {
            let layout_id = cx_ref.taffy.new_leaf(style).unwrap();

            if let Some(parent_layout_id) = cx_ref.parent_layout_id {
                cx_ref.taffy.add_child(parent_layout_id, layout_id).unwrap();
            }

            self.layout_id = Some(layout_id);
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {
        self.label.paint(ctx, scene)
    }

    fn accessibility_role(&self) -> Role {
        Role::StaticText
    }

    fn accessibility(&mut self, ctx: &mut AccessCtx) {
        self.label.accessibility(ctx)
    }

    fn children(&self) -> SmallVec<[WidgetRef<'_, dyn Widget>; 16]> {
        smallvec![self.label.as_dyn()]
    }
}
