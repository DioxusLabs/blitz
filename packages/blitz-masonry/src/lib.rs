use accesskit::Role;
use blitz_dom::{Document, DocumentHtmlParser};
use masonry::{
    vello::Scene,
    widget::{Label, WidgetRef},
    AccessCtx, AccessEvent, BoxConstraints, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    PointerEvent, Size, StatusChange, TextEvent, Widget, WidgetPod,
};
use smallvec::{smallvec, SmallVec};

mod viewport;
use self::viewport::Viewport;

pub enum Node {
    Element(WidgetPod<ElementWidget>),
    Text(WidgetPod<Label>),
}

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
                Node::Element(elem) => elem.layout(ctx, bc),
                Node::Text(text) => text.layout(ctx, bc),
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

    fn accessibility(&mut self, ctx: &mut AccessCtx) {}

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

pub struct ElementWidget {}

impl Widget for ElementWidget {
    fn on_pointer_event(&mut self, ctx: &mut EventCtx, event: &PointerEvent) {}
    fn on_text_event(&mut self, ctx: &mut EventCtx, event: &TextEvent) {}

    fn on_access_event(&mut self, ctx: &mut EventCtx, event: &AccessEvent) {}

    fn on_status_change(&mut self, ctx: &mut LifeCycleCtx, event: &StatusChange) {}

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {}

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        Default::default()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        Default::default()
    }

    fn accessibility(&mut self, ctx: &mut AccessCtx) {}

    fn children(&self) -> SmallVec<[WidgetRef<'_, dyn Widget>; 16]> {
        smallvec![]
    }
}
