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

pub struct Text {
    label: WidgetPod<Label>,
}

impl Text {
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            label: WidgetPod::new(Label::new(content)),
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
            dbg!(&cx);
            ctx.get_mut(&mut self.label)
                .set_text_size(cx.inner.borrow().font_size);
        }

        self.label.lifecycle(ctx, event);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        let size = self.label.layout(ctx, bc);
        ctx.place_child(&mut self.label, Point::ZERO);
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
