use parley::layout::PositionedLayoutItem;

use crate::Document;

impl Document {
    pub fn print_taffy_tree(&self) {
        taffy::print_tree(self, taffy::NodeId::from(0usize));
    }

    pub fn debug_log_node(&self, node_id: usize) {
        let node = &self.nodes[node_id];

        #[cfg(feature = "tracing")]
        {
            tracing::info!("Layout: {:?}", &node.final_layout);
            tracing::info!("Style: {:?}", &node.style);
        }

        println!("Node {} {}", node.id, node.node_debug_str());

        println!("Attrs:");

        for attr in node.attrs().into_iter().flatten() {
            println!("    {}: {}", attr.name.local, attr.value);
        }

        if node.is_inline_root {
            let inline_layout = &node
                .raw_dom_data
                .downcast_element()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap();

            println!(
                "Size: {}x{}",
                inline_layout.layout.width(),
                inline_layout.layout.height()
            );
            println!("Text content: {:?}", inline_layout.text);
            println!("Inline Boxes:");
            for ibox in inline_layout.layout.inline_boxes() {
                print!("(id: {}) ", ibox.id);
            }
            println!();
            println!("Lines:");
            for (i, line) in inline_layout.layout.lines().enumerate() {
                println!("Line {i}:");
                for item in line.items() {
                    print!("  ");
                    match item {
                        PositionedLayoutItem::GlyphRun(run) => {
                            print!(
                                "RUN (x: {}, w: {}) ",
                                run.offset().round(),
                                run.run().advance()
                            )
                        }
                        PositionedLayoutItem::InlineBox(ibox) => print!(
                            "BOX (id: {} x: {} y: {} w: {}, h: {})",
                            ibox.id,
                            ibox.x.round(),
                            ibox.y.round(),
                            ibox.width.round(),
                            ibox.height.round()
                        ),
                    }
                    println!();
                }
            }
        }

        println!("Parent: {:?}", node.parent);

        let children: Vec<_> = node
            .children
            .iter()
            .map(|id| &self.nodes[*id])
            .map(|node| (node.id, node.order(), node.node_debug_str()))
            .collect();
        println!("Children: {:?}", children);

        println!("Layout Parent: {:?}", node.layout_parent.get());

        let layout_children: Vec<_> = node
            .layout_children
            .borrow()
            .as_ref()
            .unwrap()
            .iter()
            .map(|id| &self.nodes[*id])
            .map(|node| (node.id, node.order(), node.node_debug_str()))
            .collect();
        println!("Layout Children: {:?}", layout_children);
        // taffy::print_tree(&self.dom, node_id.into());
    }
}
