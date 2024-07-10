use crate::{Context, Text};
use accesskit::Role;
use masonry::{
    vello::Scene, widget::WidgetRef, AccessCtx, AccessEvent, BoxConstraints, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, PointerEvent, Size, StatusChange, TextEvent, Widget,
    WidgetPod,
};
use smallvec::SmallVec;
use std::{borrow::Cow, collections::LinkedList};

pub enum Node {
    Element(WidgetPod<Element>),
    Text(WidgetPod<Text>),
}

impl From<Element> for Node {
    fn from(value: Element) -> Self {
        Node::Element(WidgetPod::new(value))
    }
}

impl From<Text> for Node {
    fn from(value: Text) -> Self {
        Node::Text(WidgetPod::new(value))
    }
}

pub struct Element {
    tag: Cow<'static, str>,
    // TODO replace with stylo styles
    pub font_size: Option<f32>,
    children: LinkedList<Node>,
}

impl Element {
    pub fn new(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            tag: tag.into(),
            font_size: None,
            children: LinkedList::new(),
        }
    }

    pub fn append_child(&mut self, child: impl Into<Node>) {
        self.children.push_back(child.into());
    }
}

impl Widget for Element {
    fn on_pointer_event(&mut self, ctx: &mut EventCtx, event: &PointerEvent) {}

    fn on_text_event(&mut self, ctx: &mut EventCtx, event: &TextEvent) {}

    fn on_access_event(&mut self, ctx: &mut EventCtx, event: &AccessEvent) {}

    fn on_status_change(&mut self, ctx: &mut LifeCycleCtx, event: &StatusChange) {}

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {
        let cx = Context::current().unwrap_or_default();
        dbg!(&cx);
        if let Some(font_size) = self.font_size {
            cx.inner.borrow_mut().font_size = font_size;
        }
        cx.enter();

        for child in &mut self.children {
            match child {
                Node::Element(elem) => elem.lifecycle(ctx, event),
                Node::Text(text) => text.lifecycle(ctx, event),
            }
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        let cx = Context::current().unwrap_or_default();
        dbg!(&cx);
        if let Some(font_size) = self.font_size {
            cx.inner.borrow_mut().font_size = font_size;
        }
        cx.enter();

        self.children
            .iter_mut()
            .map(|child| match child {
                Node::Element(elem) => {
                    let size = elem.layout(ctx, bc);
                    ctx.place_child(elem, Point::ZERO);
                    size
                }
                Node::Text(text) => {
                    let size = text.layout(ctx, bc);
                    ctx.place_child(text, Point::ZERO);
                    size
                }
            })
            .fold(Size::default(), |acc, size| acc + size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {
        for child in &mut self.children {
            match child {
                Node::Element(elem) => elem.paint(ctx, scene),
                Node::Text(text) => text.paint(ctx, scene),
            }
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Document
    }

    fn accessibility(&mut self, ctx: &mut AccessCtx) {
        for child in &mut self.children {
            match child {
                Node::Element(elem) => elem.accessibility(ctx),
                Node::Text(text) => text.accessibility(ctx),
            }
        }
    }

    fn children(&self) -> SmallVec<[WidgetRef<'_, dyn Widget>; 16]> {
        self.children
            .iter()
            .map(|child| match child {
                Node::Element(elem) => elem.as_dyn(),
                Node::Text(text) => text.as_dyn(),
            })
            .collect()
    }
}
