use blitz_traits::node_id::NodeId;

use crate::BaseDocument;
use crate::layout::TaffyDebugTree;
use crate::text::InlineText as _;

impl BaseDocument {
    pub fn print_taffy_tree(&self) {
        let root_id = crate::taffy_node_id(self.root_element().id);
        taffy::print_tree(&TaffyDebugTree(self), root_id);
        for &node_id in &self.sub_document_nodes {
            if let Some(sub_doc) = self.nodes[node_id].subdoc() {
                println!("\n=== Subdocument (node {node_id:?}) ===");
                sub_doc.inner().print_taffy_tree();
            }
        }
    }

    pub fn debug_log_node(&self, node_id: NodeId) {
        let node = &self.nodes[node_id];

        #[cfg(feature = "tracing")]
        {
            tracing::info!("Layout: {:?}", node.final_layout());
            tracing::info!("Display: {:?}", node.taffy_display());
        }

        println!("\nNode {} {}", node.id, node.node_debug_str());

        println!("Attrs:");

        for attr in node.attrs().into_iter().flatten() {
            println!("    {}: {}", attr.name.local, attr.value);
        }

        if node.flags.is_inline_root() {
            let inline_layout = &node
                .data
                .downcast_element()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap();

            inline_layout.debug_print();
        }

        let layout = node.final_layout();
        println!("Layout:");
        println!(
            "  x: {x} y: {y} w: {width} h: {height} overflow: l:{ol} r:{or} t:{ot} b:{ob}",
            x = layout.location.x,
            y = layout.location.y,
            width = layout.size.width,
            height = layout.size.height,
            ol = layout.scrollable_overflow_rect.left,
            or = layout.scrollable_overflow_rect.right,
            ot = layout.scrollable_overflow_rect.top,
            ob = layout.scrollable_overflow_rect.bottom,
        );
        println!(
            "  border: l:{l} r:{r} t:{t} b:{b}",
            l = layout.border.left,
            r = layout.border.right,
            t = layout.border.top,
            b = layout.border.bottom,
        );
        println!(
            "  padding: l:{l} r:{r} t:{t} b:{b}",
            l = layout.padding.left,
            r = layout.padding.right,
            t = layout.padding.top,
            b = layout.padding.bottom,
        );
        println!(
            "  margin: l:{l} r:{r} t:{t} b:{b}",
            l = layout.margin.left,
            r = layout.margin.right,
            t = layout.margin.top,
            b = layout.margin.bottom,
        );
        println!("Parent: {:?}", node.parent);

        let children: Vec<_> = node
            .children
            .iter()
            .map(|id| &self.nodes[*id])
            .map(|node| (node.id, node.order(), node.node_debug_str()))
            .collect();
        println!("Children: {children:?}");

        println!("Layout Parent: {:?}", node.layout_parent.get());

        let layout_children: Option<Vec<_>> = node.layout_children.borrow().as_ref().map(|lc| {
            lc.iter()
                .map(|id| &self.nodes[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect()
        });
        if let Some(layout_children) = layout_children {
            println!("Layout Children: {layout_children:?}");
        }

        let paint_children: Option<Vec<_>> = node.paint_children.borrow().as_ref().map(|lc| {
            lc.iter()
                .map(|id| &self.nodes[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect()
        });
        if let Some(paint_children) = paint_children {
            println!("Paint Children: {paint_children:?}");
        }
        // taffy::print_tree(&self.dom, node_id.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentConfig, qual_name, taffy_node_id};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use taffy::{PrintTree, TraversePartialTree};

    #[test]
    fn prints_tree_from_shared_document_reference() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html", html), vec![]);
        let body = mutator.create_element(qual_name!("body", html), vec![]);
        let child = mutator.create_element(qual_name!("div", html), vec![]);
        mutator.set_style_property(child, "height", "20px");
        mutator.append_children(body, &[child]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root, &[html]);
        drop(mutator);
        doc.resolve(0.0);

        let doc: &BaseDocument = &doc;
        doc.print_taffy_tree();

        let tree = TaffyDebugTree(doc);
        let body_id = taffy_node_id(body);
        let child_id = taffy_node_id(child);
        assert_eq!(tree.child_count(body_id), 1);
        assert_eq!(tree.get_child_id(body_id, 0), child_id);
        assert_eq!(tree.child_ids(body_id).collect::<Vec<_>>(), [child_id]);
        assert_eq!(tree.child_count(child_id), 0);
        assert_eq!(tree.child_ids(child_id).count(), 0);
        assert_eq!(tree.get_final_layout(child_id).size.height, 20.0);
        assert_eq!(
            tree.get_final_layout(child_id),
            *doc.nodes[child].final_layout()
        );

        let mut output = Vec::new();
        taffy::write_tree(&mut output, &tree, taffy_node_id(html)).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.lines().count(), 4);
        assert!(output.contains("BLOCK"));
    }
}
