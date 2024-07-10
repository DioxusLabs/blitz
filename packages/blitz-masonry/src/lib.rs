use std::{borrow::Cow, cell::RefCell, collections::LinkedList, rc::Rc};

use accesskit::Role;
use blitz_dom::{Document, DocumentHtmlParser};
use masonry::{
    vello::Scene,
    widget::{Label, WidgetRef},
    AccessCtx, AccessEvent, BoxConstraints, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    Point, PointerEvent, Size, StatusChange, TextEvent, Widget, WidgetPod,
};
use smallvec::{smallvec, SmallVec};

mod viewport;
use self::viewport::Viewport;

pub enum Node {
    Element(WidgetPod<Element>),
    Text(WidgetPod<Label>),
}

impl From<Element> for Node {
    fn from(value: Element) -> Self {
        Node::Element(WidgetPod::new(value))
    }
}

impl From<Label> for Node {
    fn from(value: Label) -> Self {
        Node::Text(WidgetPod::new(value))
    }
}

#[derive(Clone, Debug, Default)]
struct Inner {
    font_size: f32,
}

#[derive(Clone, Debug, Default)]
struct Context {
    inner: Rc<RefCell<Inner>>,
}

impl Context {
    fn current() -> Option<Self> {
        CONTEXT.try_with(|cell| cell.borrow().clone()).unwrap()
    }

    fn enter(self) -> Option<Self> {
        CONTEXT.try_with(|cell| cell.replace(Some(self))).unwrap()
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = RefCell::new(None);
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

/*
pub struct DocumentWidget {
    doc: Document,
    children: Vec<Node>,
}

impl DocumentWidget {
    pub fn from_html(html: &str) -> Self {
        let mut doc = Document::new(Viewport::new((0, 0)).make_device());
        DocumentHtmlParser::parse_into_doc(&mut doc, html);

        let mut children = Vec::new();
        doc.visit(|node_id, node| {
            let child = if node.is_text_node() {
                Node::Text(WidgetPod::new(Label::new(node.text_content())))
            } else {
                Node::Element(WidgetPod::new(ElementWidget {}))
            };
            children.push(child);
        });

        Self { doc, children }
    }
}

impl Widget for DocumentWidget {
    fn on_pointer_event(&mut self, ctx: &mut EventCtx, event: &PointerEvent) {}

    fn on_text_event(&mut self, ctx: &mut EventCtx, event: &TextEvent) {}

    fn on_access_event(&mut self, ctx: &mut EventCtx, event: &AccessEvent) {}

    fn on_status_change(&mut self, ctx: &mut LifeCycleCtx, event: &StatusChange) {}

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {
        for child in &mut self.children {
            match child {
                Node::Element(elem) => elem.lifecycle(ctx, event),
                Node::Text(text) => text.lifecycle(ctx, event),
            }
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
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
*/
