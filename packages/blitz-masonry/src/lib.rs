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
        for child_id in &doc.root_node().children {
            let child = doc.get_node(*child_id).unwrap();
            let node = if child.is_text_node() {
                Node::Text(WidgetPod::new(Label::new(child.text_content())))
            } else {
                Node::Element(WidgetPod::new(ElementWidget {}))
            };
            children.push(node);
        }

        Self { doc, children }
    }
}

impl Widget for DocumentWidget {
    fn on_pointer_event(&mut self, ctx: &mut EventCtx, event: &PointerEvent) {}
    fn on_text_event(&mut self, ctx: &mut EventCtx, event: &TextEvent) {}

    fn on_access_event(&mut self, ctx: &mut EventCtx, event: &AccessEvent) {}

    fn on_status_change(&mut self, ctx: &mut LifeCycleCtx, event: &StatusChange) {}

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {}

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        todo!()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        todo!()
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
        todo!()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        todo!()
    }

    fn accessibility(&mut self, ctx: &mut AccessCtx) {}

    fn children(&self) -> SmallVec<[WidgetRef<'_, dyn Widget>; 16]> {
        smallvec![]
    }
}
