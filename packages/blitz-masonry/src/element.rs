use crate::{Context, Text};
use accesskit::Role;
use masonry::{
    vello::Scene, widget::WidgetRef, AccessCtx, AccessEvent, BoxConstraints, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, PointerEvent, Size, StatusChange, TextEvent, Widget,
    WidgetPod,
};
use smallvec::SmallVec;
use std::{borrow::Cow, collections::LinkedList};
use taffy::{AvailableSpace, NodeId, Style};

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
    pub style: Style,
    layout_id: Option<NodeId>,
}

impl Element {
    pub fn new(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            tag: tag.into(),
            font_size: None,
            children: LinkedList::new(),
            style: Style::default(),
            layout_id: None,
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
        let (cx, is_descendant) = Context::current().map(|cx| (cx, true)).unwrap_or_default();
        let mut cx_ref = cx.inner.borrow_mut();

        if let Some(layout_id) = self.layout_id {
            cx_ref
                .taffy
                .set_style(layout_id, self.style.clone())
                .unwrap();
        } else {
            let layout_id = cx_ref.taffy.new_leaf(self.style.clone()).unwrap();

            if let Some(parent_layout_id) = cx_ref.parent_layout_id {
                cx_ref.taffy.add_child(parent_layout_id, layout_id).unwrap();
            }

            self.layout_id = Some(layout_id);
        }

        if let Some(font_size) = self.font_size {
            cx_ref.font_size = font_size;
        }

        cx_ref.parent_layout_id = self.layout_id;

        drop(cx_ref);
        cx.enter();

        let size = self
            .children
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
            .fold(Size::default(), |acc, size| acc + size);

        let cx = Context::current().unwrap();
        cx.inner
            .borrow_mut()
            .taffy
            .compute_layout(
                self.layout_id.unwrap(),
                taffy::Size {
                    width: AvailableSpace::Definite(bc.max().width as _),
                    height: AvailableSpace::Definite(bc.max().height as _),
                },
            )
            .unwrap();
        let layout = cx
            .inner
            .borrow()
            .taffy
            .layout(self.layout_id.unwrap())
            .unwrap()
            .clone();

        dbg!(&layout);

        Size::new(layout.size.width as _, layout.size.height as _)
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
